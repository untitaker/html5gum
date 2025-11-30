use crate::entities::try_read_character_reference;
use crate::machine_helper::{
    cont, emit_current_tag_and_switch_to, enter_state, eof, error, error_immediate, exit_state,
    mutate_character_reference, read_byte, reconsume_in, reconsume_in_return_state, switch_to,
    ControlToken,
};
use crate::read_helper::{fast_read_char, slow_read_byte};
use crate::utils::{ctostr, noncharacter_pat, surrogate_pat, with_lowercase_str};
use crate::{Emitter, Error, Reader, Tokenizer};

macro_rules! define_state {
    ($state:ident, $slf:ident, $($body:tt)*) => {
        #[allow(non_snake_case)]
        pub(crate) mod $state {
            use super::*;

            #[inline(always)]
            pub(crate) fn run<R: Reader, E: Emitter>($slf: &mut Tokenizer<R, E>) -> Result<ControlToken<R, E>, R::Error> {
                $($body)*
            }
        }
    };
}

pub(crate) mod states {
    use super::*;

    define_state!(Data, slf, {
        slf.emitter.init_string();
        fast_read_char!(
            slf,
            match xs {
                Some(b"&") => {
                    enter_state!(slf, CharacterReference, false)
                }
                Some(b"<") => {
                    slf.emitter.start_open_tag();
                    switch_to!(slf, TagOpen)?.inline_next_state(slf)
                }
                Some(b"\0") => {
                    error!(slf, Error::UnexpectedNullCharacter);
                    slf.emitter.emit_string(b"\0");
                    cont!()
                }
                Some(xs) => {
                    slf.emitter.emit_string(xs);
                    cont!()
                }
                None => {
                    eof!()
                }
            }
        )
    });

    define_state!(RcData, slf, {
        slf.emitter.init_string();
        fast_read_char!(
            slf,
            match xs {
                Some(b"&") => {
                    enter_state!(slf, CharacterReference, false)
                }
                Some(b"<") => {
                    slf.emitter.start_open_tag();
                    switch_to!(slf, RcDataLessThanSign)
                }
                Some(b"\0") => {
                    error!(slf, Error::UnexpectedNullCharacter);
                    slf.emitter.emit_string("\u{fffd}".as_bytes());
                    cont!()
                }
                Some(xs) => {
                    slf.emitter.emit_string(xs);
                    cont!()
                }
                None => {
                    eof!()
                }
            }
        )
    });

    define_state!(RawText, slf, {
        fast_read_char!(
            slf,
            match xs {
                Some(b"<") => {
                    slf.emitter.start_open_tag();
                    switch_to!(slf, RawTextLessThanSign)
                }
                Some(b"\0") => {
                    error!(slf, Error::UnexpectedNullCharacter);
                    slf.emitter.emit_string("\u{fffd}".as_bytes());
                    cont!()
                }
                Some(xs) => {
                    slf.emitter.emit_string(xs);
                    cont!()
                }
                None => {
                    eof!()
                }
            }
        )
    });

    define_state!(ScriptData, slf, {
        fast_read_char!(
            slf,
            match xs {
                Some(b"<") => {
                    slf.emitter.start_open_tag();
                    switch_to!(slf, ScriptDataLessThanSign)
                }
                Some(b"\0") => {
                    error!(slf, Error::UnexpectedNullCharacter);
                    slf.emitter.emit_string("\u{fffd}".as_bytes());
                    cont!()
                }
                Some(xs) => {
                    slf.emitter.emit_string(xs);
                    cont!()
                }
                None => {
                    eof!()
                }
            }
        )
    });

    define_state!(PlainText, slf, {
        fast_read_char!(
            slf,
            match xs {
                Some(b"\0") => {
                    error!(slf, Error::UnexpectedNullCharacter);
                    slf.emitter.emit_string("\u{fffd}".as_bytes());
                    cont!()
                }
                Some(xs) => {
                    slf.emitter.emit_string(xs);
                    cont!()
                }
                None => {
                    eof!()
                }
            }
        )
    });

    define_state!(TagOpen, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'!') => {
                    switch_to!(slf, MarkupDeclarationOpen)
                }
                Some(b'/') => {
                    switch_to!(slf, EndTagOpen)?.inline_next_state(slf)
                }
                Some(x) if x.is_ascii_alphabetic() => {
                    slf.emitter.init_start_tag();
                    reconsume_in!(slf, Some(x), TagName)?.inline_next_state(slf)
                }
                c @ Some(b'?') => {
                    error!(slf, Error::UnexpectedQuestionMarkInsteadOfTagName);
                    slf.emitter.init_comment();
                    reconsume_in!(slf, c, BogusComment)
                }
                None => {
                    error!(slf, Error::EofBeforeTagName);
                    slf.emitter.emit_string(b"<");
                    eof!()
                }
                c @ Some(_) => {
                    error!(slf, Error::InvalidFirstCharacterOfTagName);
                    slf.emitter.move_position(-1);
                    slf.emitter.emit_string(b"<");
                    slf.emitter.move_position(1);
                    reconsume_in!(slf, c, Data)
                }
            }
        )
    });

    define_state!(EndTagOpen, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(x) if x.is_ascii_alphabetic() => {
                    slf.emitter.init_end_tag();
                    reconsume_in!(slf, Some(x), TagName)
                }
                Some(b'>') => {
                    error!(slf, Error::MissingEndTagName);
                    switch_to!(slf, Data)
                }
                None => {
                    error!(slf, Error::EofBeforeTagName);
                    slf.emitter.emit_string(b"</");
                    eof!()
                }
                Some(x) => {
                    error!(slf, Error::InvalidFirstCharacterOfTagName);
                    slf.emitter.init_comment();
                    reconsume_in!(slf, Some(x), BogusComment)
                }
            }
        )
    });

    define_state!(TagName, slf, {
        fast_read_char!(
            slf,
            match xs {
                Some(b"\t" | b"\x0A" | b"\x0C" | b" ") => {
                    switch_to!(slf, BeforeAttributeName)
                }
                Some(b"/") => {
                    switch_to!(slf, SelfClosingStartTag)
                }
                Some(b">") => {
                    // candidate for inline_next_state except it'd be cyclic
                    emit_current_tag_and_switch_to!(slf, Data)
                }
                Some(b"\0") => {
                    error!(slf, Error::UnexpectedNullCharacter);
                    slf.emitter.push_tag_name("\u{fffd}".as_bytes());
                    cont!()
                }
                Some(xs) => {
                    let emitter = &mut slf.emitter;
                    with_lowercase_str(xs, |x| {
                        emitter.push_tag_name(x);
                    });

                    cont!()
                }
                None => {
                    error!(slf, Error::EofInTag);
                    eof!()
                }
            }
        )
    });

    define_state!(RcDataLessThanSign, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'/') => {
                    slf.machine_helper.temporary_buffer.clear();
                    switch_to!(slf, RcDataEndTagOpen)
                }
                c => {
                    slf.emitter.emit_string(b"<");
                    reconsume_in!(slf, c, RcData)
                }
            }
        )
    });

    define_state!(RcDataEndTagOpen, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(x) if x.is_ascii_alphabetic() => {
                    slf.emitter.init_end_tag();
                    reconsume_in!(slf, Some(x), RcDataEndTagName)
                }
                c => {
                    slf.emitter.emit_string(b"</");
                    reconsume_in!(slf, c, RcData)
                }
            }
        )
    });

    define_state!(RcDataEndTagName, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ')
                    if slf.emitter.current_is_appropriate_end_tag_token() =>
                {
                    switch_to!(slf, BeforeAttributeName)
                }
                Some(b'/') if slf.emitter.current_is_appropriate_end_tag_token() => {
                    switch_to!(slf, SelfClosingStartTag)
                }
                Some(b'>') if slf.emitter.current_is_appropriate_end_tag_token() => {
                    emit_current_tag_and_switch_to!(slf, Data)
                }
                Some(x) if x.is_ascii_alphabetic() => {
                    slf.emitter.push_tag_name(&[x.to_ascii_lowercase()]);
                    slf.machine_helper.temporary_buffer.push(x);
                    cont!()
                }
                c => {
                    slf.emitter.emit_string(b"</");
                    slf.machine_helper.flush_buffer_characters(&mut slf.emitter);
                    reconsume_in!(slf, c, RcData)
                }
            }
        )
    });

    define_state!(RawTextLessThanSign, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'/') => {
                    slf.machine_helper.temporary_buffer.clear();
                    switch_to!(slf, RawTextEndTagOpen)
                }
                c => {
                    slf.emitter.emit_string(b"<");
                    reconsume_in!(slf, c, RawText)
                }
            }
        )
    });

    define_state!(RawTextEndTagOpen, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(x) if x.is_ascii_alphabetic() => {
                    slf.emitter.init_end_tag();
                    reconsume_in!(slf, Some(x), RawTextEndTagName)
                }
                c => {
                    slf.emitter.emit_string(b"</");
                    reconsume_in!(slf, c, RawText)
                }
            }
        )
    });

    define_state!(RawTextEndTagName, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ')
                    if slf.emitter.current_is_appropriate_end_tag_token() =>
                {
                    switch_to!(slf, BeforeAttributeName)
                }
                Some(b'/') if slf.emitter.current_is_appropriate_end_tag_token() => {
                    switch_to!(slf, SelfClosingStartTag)
                }
                Some(b'>') if slf.emitter.current_is_appropriate_end_tag_token() => {
                    emit_current_tag_and_switch_to!(slf, Data)
                }
                Some(x) if x.is_ascii_alphabetic() => {
                    slf.emitter.push_tag_name(&[x.to_ascii_lowercase()]);
                    slf.machine_helper.temporary_buffer.push(x);
                    cont!()
                }
                c => {
                    slf.emitter.emit_string(b"</");
                    slf.machine_helper.flush_buffer_characters(&mut slf.emitter);
                    reconsume_in!(slf, c, RawText)
                }
            }
        )
    });

    define_state!(ScriptDataLessThanSign, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'/') => {
                    slf.machine_helper.temporary_buffer.clear();
                    switch_to!(slf, ScriptDataEndTagOpen)
                }
                Some(b'!') => {
                    slf.emitter.emit_string(b"<!");
                    switch_to!(slf, ScriptDataEscapeStart)
                }
                c => {
                    slf.emitter.emit_string(b"<");
                    reconsume_in!(slf, c, ScriptData)
                }
            }
        )
    });

    define_state!(ScriptDataEndTagOpen, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(x) if x.is_ascii_alphabetic() => {
                    slf.emitter.init_end_tag();
                    reconsume_in!(slf, Some(x), ScriptDataEndTagName)
                }
                c => {
                    slf.emitter.emit_string(b"</");
                    reconsume_in!(slf, c, ScriptData)
                }
            }
        )
    });

    define_state!(ScriptDataEndTagName, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ')
                    if slf.emitter.current_is_appropriate_end_tag_token() =>
                {
                    switch_to!(slf, BeforeAttributeName)
                }
                Some(b'/') if slf.emitter.current_is_appropriate_end_tag_token() => {
                    switch_to!(slf, SelfClosingStartTag)
                }
                Some(b'>') if slf.emitter.current_is_appropriate_end_tag_token() => {
                    emit_current_tag_and_switch_to!(slf, Data)
                }
                Some(x) if x.is_ascii_alphabetic() => {
                    slf.emitter.push_tag_name(&[x.to_ascii_lowercase()]);
                    slf.machine_helper.temporary_buffer.push(x);
                    cont!()
                }
                c => {
                    slf.emitter.emit_string(b"</");
                    slf.machine_helper.flush_buffer_characters(&mut slf.emitter);
                    reconsume_in!(slf, c, ScriptData)
                }
            }
        )
    });

    define_state!(ScriptDataEscapeStart, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'-') => {
                    slf.emitter.emit_string(b"-");
                    switch_to!(slf, ScriptDataEscapeStartDash)
                }
                c => {
                    reconsume_in!(slf, c, ScriptData)
                }
            }
        )
    });

    define_state!(ScriptDataEscapeStartDash, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'-') => {
                    slf.emitter.emit_string(b"-");
                    switch_to!(slf, ScriptDataEscapedDashDash)
                }
                c => {
                    reconsume_in!(slf, c, ScriptData)
                }
            }
        )
    });

    define_state!(ScriptDataEscaped, slf, {
        fast_read_char!(
            slf,
            match xs {
                Some(b"-") => {
                    slf.emitter.emit_string(b"-");
                    switch_to!(slf, ScriptDataEscapedDash)
                }
                Some(b"<") => {
                    switch_to!(slf, ScriptDataEscapedLessThanSign)
                }
                Some(b"\0") => {
                    error!(slf, Error::UnexpectedNullCharacter);
                    slf.emitter.emit_string("\u{fffd}".as_bytes());
                    cont!()
                }
                Some(xs) => {
                    slf.emitter.emit_string(xs);
                    cont!()
                }
                None => {
                    error!(slf, Error::EofInScriptHtmlCommentLikeText);
                    eof!()
                }
            }
        )
    });

    define_state!(ScriptDataEscapedDash, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'-') => {
                    slf.emitter.emit_string(b"-");
                    switch_to!(slf, ScriptDataEscapedDashDash)
                }
                Some(b'<') => {
                    switch_to!(slf, ScriptDataEscapedLessThanSign)
                }
                Some(b'\0') => {
                    error!(slf, Error::UnexpectedNullCharacter);
                    slf.emitter.emit_string("\u{fffd}".as_bytes());
                    switch_to!(slf, ScriptDataEscaped)
                }
                Some(x) => {
                    slf.emitter.emit_string(&[x]);
                    switch_to!(slf, ScriptDataEscaped)
                }
                None => {
                    error!(slf, Error::EofInScriptHtmlCommentLikeText);
                    eof!()
                }
            }
        )
    });

    define_state!(ScriptDataEscapedDashDash, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'-') => {
                    slf.emitter.emit_string(b"-");
                    cont!()
                }
                Some(b'<') => {
                    switch_to!(slf, ScriptDataEscapedLessThanSign)
                }
                Some(b'>') => {
                    slf.emitter.emit_string(b">");
                    switch_to!(slf, ScriptData)
                }
                Some(b'\0') => {
                    error!(slf, Error::UnexpectedNullCharacter);
                    slf.emitter.emit_string("\u{fffd}".as_bytes());
                    switch_to!(slf, ScriptDataEscaped)
                }
                Some(x) => {
                    slf.emitter.emit_string(&[x]);
                    switch_to!(slf, ScriptDataEscaped)
                }
                None => {
                    error!(slf, Error::EofInScriptHtmlCommentLikeText);
                    eof!()
                }
            }
        )
    });

    define_state!(ScriptDataEscapedLessThanSign, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'/') => {
                    slf.machine_helper.temporary_buffer.clear();
                    switch_to!(slf, ScriptDataEscapedEndTagOpen)
                }
                Some(x) if x.is_ascii_alphabetic() => {
                    slf.machine_helper.temporary_buffer.clear();
                    slf.emitter.emit_string(b"<");
                    reconsume_in!(slf, Some(x), ScriptDataDoubleEscapeStart)
                }
                c => {
                    slf.emitter.emit_string(b"<");
                    reconsume_in!(slf, c, ScriptDataEscaped)
                }
            }
        )
    });

    define_state!(ScriptDataEscapedEndTagOpen, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(x) if x.is_ascii_alphabetic() => {
                    slf.emitter.init_end_tag();
                    reconsume_in!(slf, Some(x), ScriptDataEscapedEndTagName)
                }
                c => {
                    slf.emitter.emit_string(b"</");
                    reconsume_in!(slf, c, ScriptDataEscaped)
                }
            }
        )
    });

    define_state!(ScriptDataEscapedEndTagName, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ')
                    if slf.emitter.current_is_appropriate_end_tag_token() =>
                {
                    switch_to!(slf, BeforeAttributeName)
                }
                Some(b'/') if slf.emitter.current_is_appropriate_end_tag_token() => {
                    switch_to!(slf, SelfClosingStartTag)
                }
                Some(b'>') if slf.emitter.current_is_appropriate_end_tag_token() => {
                    emit_current_tag_and_switch_to!(slf, Data)
                }
                Some(x) if x.is_ascii_alphabetic() => {
                    slf.emitter.push_tag_name(&[x.to_ascii_lowercase()]);
                    slf.machine_helper.temporary_buffer.extend(&[x]);
                    cont!()
                }
                c => {
                    slf.emitter.emit_string(b"</");
                    slf.machine_helper.flush_buffer_characters(&mut slf.emitter);
                    reconsume_in!(slf, c, ScriptDataEscaped)
                }
            }
        )
    });

    define_state!(ScriptDataDoubleEscapeStart, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(x @ (b'\t' | b'\x0A' | b'\x0C' | b' ' | b'/' | b'>')) => {
                    slf.emitter.emit_string(&[x]);
                    if slf.machine_helper.temporary_buffer == b"script" {
                        switch_to!(slf, ScriptDataDoubleEscaped)
                    } else {
                        switch_to!(slf, ScriptDataEscaped)
                    }
                }
                Some(x) if x.is_ascii_alphabetic() => {
                    slf.machine_helper
                        .temporary_buffer
                        .push(x.to_ascii_lowercase());
                    slf.emitter.emit_string(&[x]);
                    cont!()
                }
                c => {
                    reconsume_in!(slf, c, ScriptDataEscaped)
                }
            }
        )
    });

    define_state!(ScriptDataDoubleEscaped, slf, {
        fast_read_char!(
            slf,
            match xs {
                Some(b"-") => {
                    slf.emitter.emit_string(b"-");
                    switch_to!(slf, ScriptDataDoubleEscapedDash)
                }
                Some(b"<") => {
                    slf.emitter.emit_string(b"<");
                    switch_to!(slf, ScriptDataDoubleEscapedLessThanSign)
                }
                Some(b"\0") => {
                    error!(slf, Error::UnexpectedNullCharacter);
                    slf.emitter.emit_string("\u{fffd}".as_bytes());
                    cont!()
                }
                Some(xs) => {
                    slf.emitter.emit_string(xs);
                    cont!()
                }
                None => {
                    error!(slf, Error::EofInScriptHtmlCommentLikeText);
                    eof!()
                }
            }
        )
    });

    define_state!(ScriptDataDoubleEscapedDash, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'-') => {
                    slf.emitter.emit_string(b"-");
                    switch_to!(slf, ScriptDataDoubleEscapedDashDash)
                }
                Some(b'<') => {
                    slf.emitter.emit_string(b"<");
                    switch_to!(slf, ScriptDataDoubleEscapedLessThanSign)
                }
                Some(b'\0') => {
                    error!(slf, Error::UnexpectedNullCharacter);
                    slf.emitter.emit_string("\u{fffd}".as_bytes());
                    switch_to!(slf, ScriptDataDoubleEscaped)
                }
                Some(x) => {
                    slf.emitter.emit_string(&[x]);
                    switch_to!(slf, ScriptDataDoubleEscaped)
                }
                None => {
                    error!(slf, Error::EofInScriptHtmlCommentLikeText);
                    eof!()
                }
            }
        )
    });

    define_state!(ScriptDataDoubleEscapedDashDash, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'-') => {
                    slf.emitter.emit_string(b"-");
                    cont!()
                }
                Some(b'<') => {
                    slf.emitter.emit_string(b"<");
                    switch_to!(slf, ScriptDataDoubleEscapedLessThanSign)
                }
                Some(b'>') => {
                    slf.emitter.emit_string(b">");
                    switch_to!(slf, ScriptData)
                }
                Some(b'\0') => {
                    error!(slf, Error::UnexpectedNullCharacter);
                    slf.emitter.emit_string("\u{fffd}".as_bytes());
                    switch_to!(slf, ScriptDataDoubleEscaped)
                }
                Some(x) => {
                    slf.emitter.emit_string(&[x]);
                    switch_to!(slf, ScriptDataDoubleEscaped)
                }
                None => {
                    error!(slf, Error::EofInScriptHtmlCommentLikeText);
                    eof!()
                }
            }
        )
    });

    define_state!(ScriptDataDoubleEscapedLessThanSign, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'/') => {
                    slf.machine_helper.temporary_buffer.clear();
                    slf.emitter.emit_string(b"/");
                    switch_to!(slf, ScriptDataDoubleEscapeEnd)
                }
                c => {
                    reconsume_in!(slf, c, ScriptDataDoubleEscaped)
                }
            }
        )
    });

    define_state!(ScriptDataDoubleEscapeEnd, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(x @ (b'\t' | b'\x0A' | b'\x0C' | b' ' | b'/' | b'>')) => {
                    slf.emitter.emit_string(&[x]);

                    if slf.machine_helper.temporary_buffer == b"script" {
                        switch_to!(slf, ScriptDataEscaped)
                    } else {
                        switch_to!(slf, ScriptDataDoubleEscaped)
                    }
                }
                Some(x) if x.is_ascii_alphabetic() => {
                    slf.machine_helper
                        .temporary_buffer
                        .push(x.to_ascii_lowercase());
                    slf.emitter.emit_string(&[x]);
                    cont!()
                }
                c => {
                    reconsume_in!(slf, c, ScriptDataDoubleEscaped)
                }
            }
        )
    });

    define_state!(BeforeAttributeName, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => cont!(),
                c @ (Some(b'/' | b'>') | None) => {
                    reconsume_in!(slf, c, AfterAttributeName)?.inline_next_state(slf)
                }
                Some(b'=') => {
                    error!(slf, Error::UnexpectedEqualsSignBeforeAttributeName);
                    slf.emitter.init_attribute();
                    slf.emitter.push_attribute_name(b"=");
                    switch_to!(slf, AttributeName)
                }
                Some(x) => {
                    slf.emitter.init_attribute();
                    reconsume_in!(slf, Some(x), AttributeName)?.inline_next_state(slf)
                }
            }
        )
    });

    define_state!(AttributeName, slf, {
        fast_read_char!(
            slf,
            match xs {
                Some(b"\t" | b"\x0A" | b"\x0C" | b" " | b"/" | b">") => {
                    reconsume_in!(slf, Some(xs.unwrap()[0]), AfterAttributeName)
                }
                Some(b"=") => {
                    switch_to!(slf, BeforeAttributeValue)?.inline_next_state(slf)
                }
                Some(b"\0") => {
                    error!(slf, Error::UnexpectedNullCharacter);
                    slf.emitter.push_attribute_name("\u{fffd}".as_bytes());
                    cont!()
                }
                Some(b"\"" | b"'" | b"<") => {
                    error!(slf, Error::UnexpectedCharacterInAttributeName);
                    slf.emitter.push_attribute_name(xs.unwrap());
                    cont!()
                }
                Some(xs) => {
                    let emitter = &mut slf.emitter;
                    with_lowercase_str(xs, |xs| {
                        emitter.push_attribute_name(xs);
                    });
                    cont!()
                }
                None => {
                    reconsume_in!(slf, None, AfterAttributeName)
                }
            }
        )
    });

    define_state!(AfterAttributeName, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => cont!(),
                Some(b'/') => {
                    switch_to!(slf, SelfClosingStartTag)
                }
                Some(b'=') => {
                    switch_to!(slf, BeforeAttributeValue)
                }
                Some(b'>') => {
                    emit_current_tag_and_switch_to!(slf, Data)
                }
                None => {
                    error!(slf, Error::EofInTag);
                    eof!()
                }
                Some(x) => {
                    slf.emitter.init_attribute();
                    reconsume_in!(slf, Some(x), AttributeName)
                }
            }
        )
    });

    define_state!(BeforeAttributeValue, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => cont!(),
                Some(b'"') => {
                    slf.emitter.init_attribute_value();
                    switch_to!(slf, AttributeValueDoubleQuoted)?.inline_next_state(slf)
                }
                Some(b'\'') => {
                    slf.emitter.init_attribute_value();
                    switch_to!(slf, AttributeValueSingleQuoted)
                }
                Some(b'>') => {
                    error!(slf, Error::MissingAttributeValue);
                    emit_current_tag_and_switch_to!(slf, Data)
                }
                c => {
                    slf.emitter.init_attribute_value();
                    reconsume_in!(slf, c, AttributeValueUnquoted)
                }
            }
        )
    });

    define_state!(AttributeValueDoubleQuoted, slf, {
        fast_read_char!(
            slf,
            match xs {
                Some(b"\"") => {
                    switch_to!(slf, AfterAttributeValueQuoted)?.inline_next_state(slf)
                }
                Some(b"&") => {
                    enter_state!(slf, CharacterReference, true)
                }
                Some(b"\0") => {
                    error!(slf, Error::UnexpectedNullCharacter);
                    slf.emitter.push_attribute_value("\u{fffd}".as_bytes());
                    cont!()
                }
                Some(xs) => {
                    slf.emitter.push_attribute_value(xs);
                    cont!()
                }
                None => {
                    error!(slf, Error::EofInTag);
                    eof!()
                }
            }
        )
    });

    define_state!(AttributeValueSingleQuoted, slf, {
        fast_read_char!(
            slf,
            match xs {
                Some(b"'") => {
                    switch_to!(slf, AfterAttributeValueQuoted)
                }
                Some(b"&") => {
                    enter_state!(slf, CharacterReference, true)
                }
                Some(b"\0") => {
                    error!(slf, Error::UnexpectedNullCharacter);
                    slf.emitter.push_attribute_value("\u{fffd}".as_bytes());
                    cont!()
                }
                Some(xs) => {
                    slf.emitter.push_attribute_value(xs);
                    cont!()
                }
                None => {
                    error!(slf, Error::EofInTag);
                    eof!()
                }
            }
        )
    });

    define_state!(AttributeValueUnquoted, slf, {
        fast_read_char!(
            slf,
            match xs {
                Some(b"\t" | b"\x0A" | b"\x0C" | b" ") => {
                    switch_to!(slf, BeforeAttributeName)
                }
                Some(b"&") => {
                    enter_state!(slf, CharacterReference, true)
                }
                Some(b">") => {
                    emit_current_tag_and_switch_to!(slf, Data)
                }
                Some(b"\0") => {
                    error!(slf, Error::UnexpectedNullCharacter);
                    slf.emitter.push_attribute_value("\u{fffd}".as_bytes());
                    cont!()
                }
                Some(b"\"" | b"'" | b"<" | b"=" | b"\x60") => {
                    error!(slf, Error::UnexpectedCharacterInUnquotedAttributeValue);
                    slf.emitter.push_attribute_value(xs.unwrap());
                    cont!()
                }
                Some(xs) => {
                    slf.emitter.push_attribute_value(xs);
                    cont!()
                }
                None => {
                    error!(slf, Error::EofInTag);
                    eof!()
                }
            }
        )
    });

    define_state!(AfterAttributeValueQuoted, slf, {
        slow_read_byte!(
            slf,
            match c {
                c @ (Some(b'\t' | b'\x0A' | b'\x0C' | b' ' | b'/' | b'>') | None) => {
                    reconsume_in!(slf, c, BeforeAttributeName)?.inline_next_state(slf)
                }
                c => {
                    error!(slf, Error::MissingWhitespaceBetweenAttributes);
                    reconsume_in!(slf, c, BeforeAttributeName)
                }
            }
        )
    });

    define_state!(SelfClosingStartTag, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'>') => {
                    slf.emitter.set_self_closing();
                    emit_current_tag_and_switch_to!(slf, Data)
                }
                None => {
                    error!(slf, Error::EofInTag);
                    eof!()
                }
                Some(x) => {
                    error_immediate!(slf, Error::UnexpectedSolidusInTag);
                    reconsume_in!(slf, Some(x), BeforeAttributeName)
                }
            }
        )
    });

    define_state!(BogusComment, slf, {
        fast_read_char!(
            slf,
            match xs {
                Some(b">") => {
                    slf.emitter.emit_current_comment();
                    switch_to!(slf, Data)
                }
                Some(b"\0") => {
                    error!(slf, Error::UnexpectedNullCharacter);
                    slf.emitter.push_comment("\u{fffd}".as_bytes());
                    cont!()
                }
                Some(xs) => {
                    slf.emitter.push_comment(xs);
                    cont!()
                }
                None => {
                    slf.emitter.emit_current_comment();
                    eof!()
                }
            }
        )
    });

    define_state!(MarkupDeclarationOpen, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'-') if slf.try_read_string("-", true)? => {
                    slf.emitter.init_comment();
                    switch_to!(slf, CommentStart)
                }
                Some(b'd' | b'D') if slf.try_read_string("octype", false)? => {
                    switch_to!(slf, Doctype)
                }
                Some(b'[') if slf.try_read_string("CDATA[", true)? => {
                    if slf
                        .emitter
                        .adjusted_current_node_present_but_not_in_html_namespace()
                    {
                        switch_to!(slf, CdataSection)
                    } else {
                        error!(slf, Error::CdataInHtmlContent);

                        slf.emitter.init_comment();
                        slf.emitter.push_comment(b"[CDATA[");
                        switch_to!(slf, BogusComment)
                    }
                }
                c => {
                    error!(slf, Error::IncorrectlyOpenedComment);
                    slf.emitter.init_comment();
                    reconsume_in!(slf, c, BogusComment)
                }
            }
        )
    });

    define_state!(CommentStart, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'-') => {
                    switch_to!(slf, CommentStartDash)
                }
                Some(b'>') => {
                    error!(slf, Error::AbruptClosingOfEmptyComment);
                    slf.emitter.emit_current_comment();
                    switch_to!(slf, Data)
                }
                c => {
                    reconsume_in!(slf, c, Comment)
                }
            }
        )
    });

    define_state!(CommentStartDash, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'-') => {
                    switch_to!(slf, CommentEnd)
                }
                Some(b'>') => {
                    error!(slf, Error::AbruptClosingOfEmptyComment);
                    slf.emitter.emit_current_comment();
                    switch_to!(slf, Data)
                }
                None => {
                    error!(slf, Error::EofInComment);
                    slf.emitter.emit_current_comment();
                    eof!()
                }
                c @ Some(_) => {
                    slf.emitter.push_comment(b"-");
                    reconsume_in!(slf, c, Comment)
                }
            }
        )
    });

    define_state!(Comment, slf, {
        fast_read_char!(
            slf,
            match xs {
                Some(b"<") => {
                    slf.emitter.push_comment(b"<");
                    switch_to!(slf, CommentLessThanSign)
                }
                Some(b"-") => {
                    switch_to!(slf, CommentEndDash)
                }
                Some(b"\0") => {
                    error!(slf, Error::UnexpectedNullCharacter);
                    slf.emitter.push_comment("\u{fffd}".as_bytes());
                    cont!()
                }
                Some(xs) => {
                    slf.emitter.push_comment(xs);
                    cont!()
                }
                None => {
                    error!(slf, Error::EofInComment);
                    slf.emitter.emit_current_comment();
                    eof!()
                }
            }
        )
    });

    define_state!(CommentLessThanSign, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'!') => {
                    slf.emitter.push_comment(b"!");
                    switch_to!(slf, CommentLessThanSignBang)
                }
                Some(b'<') => {
                    slf.emitter.push_comment(b"<");
                    cont!()
                }
                c => {
                    reconsume_in!(slf, c, Comment)
                }
            }
        )
    });

    define_state!(CommentLessThanSignBang, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'-') => {
                    switch_to!(slf, CommentLessThanSignBangDash)
                }
                c => {
                    reconsume_in!(slf, c, Comment)
                }
            }
        )
    });

    define_state!(CommentLessThanSignBangDash, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'-') => {
                    switch_to!(slf, CommentLessThanSignBangDashDash)
                }
                c => {
                    reconsume_in!(slf, c, CommentEndDash)
                }
            }
        )
    });

    define_state!(CommentLessThanSignBangDashDash, slf, {
        slow_read_byte!(
            slf,
            match c {
                c @ (Some(b'>') | None) => {
                    reconsume_in!(slf, c, CommentEnd)
                }
                c => {
                    error!(slf, Error::NestedComment);
                    reconsume_in!(slf, c, CommentEnd)
                }
            }
        )
    });

    define_state!(CommentEndDash, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'-') => {
                    switch_to!(slf, CommentEnd)
                }
                None => {
                    error!(slf, Error::EofInComment);
                    slf.emitter.emit_current_comment();
                    eof!()
                }
                c => {
                    slf.emitter.push_comment(b"-");
                    reconsume_in!(slf, c, Comment)
                }
            }
        )
    });

    define_state!(CommentEnd, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'>') => {
                    slf.emitter.emit_current_comment();
                    switch_to!(slf, Data)
                }
                Some(b'!') => {
                    switch_to!(slf, CommentEndBang)
                }
                Some(b'-') => {
                    slf.emitter.push_comment(b"-");
                    cont!()
                }
                None => {
                    error!(slf, Error::EofInComment);
                    slf.emitter.emit_current_comment();
                    eof!()
                }
                c @ Some(_) => {
                    slf.emitter.push_comment(b"--");
                    reconsume_in!(slf, c, Comment)
                }
            }
        )
    });

    define_state!(CommentEndBang, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'-') => {
                    slf.emitter.push_comment(b"--!");
                    switch_to!(slf, CommentEndDash)
                }
                Some(b'>') => {
                    error!(slf, Error::IncorrectlyClosedComment);
                    slf.emitter.emit_current_comment();
                    switch_to!(slf, Data)
                }
                None => {
                    error!(slf, Error::EofInComment);
                    slf.emitter.emit_current_comment();
                    eof!()
                }
                c @ Some(_) => {
                    slf.emitter.push_comment(b"--!");
                    reconsume_in!(slf, c, Comment)
                }
            }
        )
    });

    define_state!(Doctype, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => {
                    switch_to!(slf, BeforeDoctypeName)
                }
                c @ Some(b'>') => {
                    reconsume_in!(slf, c, BeforeDoctypeName)
                }
                None => {
                    error!(slf, Error::EofInDoctype);
                    slf.emitter.init_doctype();
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    eof!()
                }
                c @ Some(_) => {
                    error!(slf, Error::MissingWhitespaceBeforeDoctypeName);
                    reconsume_in!(slf, c, BeforeDoctypeName)
                }
            }
        )
    });

    define_state!(BeforeDoctypeName, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => cont!(),
                Some(b'\0') => {
                    error!(slf, Error::UnexpectedNullCharacter);
                    slf.emitter.init_doctype();
                    slf.emitter.push_doctype_name("\u{fffd}".as_bytes());
                    switch_to!(slf, DoctypeName)
                }
                Some(b'>') => {
                    error!(slf, Error::MissingDoctypeName);
                    slf.emitter.init_doctype();
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    switch_to!(slf, Data)
                }
                None => {
                    error!(slf, Error::EofInDoctype);
                    slf.emitter.init_doctype();
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    eof!()
                }
                Some(x) => {
                    slf.emitter.init_doctype();
                    slf.emitter.push_doctype_name(&[x.to_ascii_lowercase()]);
                    switch_to!(slf, DoctypeName)
                }
            }
        )
    });

    define_state!(DoctypeName, slf, {
        fast_read_char!(
            slf,
            match xs {
                Some(b"\t" | b"\x0A" | b"\x0C" | b" ") => {
                    switch_to!(slf, AfterDoctypeName)
                }
                Some(b">") => {
                    slf.emitter.emit_current_doctype();
                    switch_to!(slf, Data)
                }
                Some(b"\0") => {
                    error!(slf, Error::UnexpectedNullCharacter);
                    slf.emitter.push_doctype_name("\u{fffd}".as_bytes());
                    cont!()
                }
                Some(xs) => {
                    let emitter = &mut slf.emitter;
                    with_lowercase_str(xs, |x| {
                        emitter.push_doctype_name(x);
                    });
                    cont!()
                }
                None => {
                    error!(slf, Error::EofInDoctype);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    eof!()
                }
            }
        )
    });

    define_state!(AfterDoctypeName, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => cont!(),
                Some(b'>') => {
                    slf.emitter.emit_current_doctype();
                    switch_to!(slf, Data)
                }
                None => {
                    error!(slf, Error::EofInDoctype);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    eof!()
                }
                Some(b'p' | b'P') if slf.try_read_string("ublic", false)? => {
                    switch_to!(slf, AfterDoctypePublicKeyword)
                }
                Some(b's' | b'S') if slf.try_read_string("ystem", false)? => {
                    switch_to!(slf, AfterDoctypeSystemKeyword)
                }
                c @ Some(_) => {
                    error!(slf, Error::InvalidCharacterSequenceAfterDoctypeName);
                    slf.emitter.set_force_quirks();
                    reconsume_in!(slf, c, BogusDoctype)
                }
            }
        )
    });

    define_state!(AfterDoctypePublicKeyword, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => {
                    switch_to!(slf, BeforeDoctypePublicIdentifier)
                }
                Some(b'"') => {
                    error!(slf, Error::MissingWhitespaceAfterDoctypePublicKeyword);
                    slf.emitter.set_doctype_public_identifier(b"");
                    switch_to!(slf, DoctypePublicIdentifierDoubleQuoted)
                }
                Some(b'\'') => {
                    error!(slf, Error::MissingWhitespaceAfterDoctypePublicKeyword);
                    slf.emitter.set_doctype_public_identifier(b"");
                    switch_to!(slf, DoctypePublicIdentifierSingleQuoted)
                }
                Some(b'>') => {
                    error!(slf, Error::MissingDoctypePublicIdentifier);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    switch_to!(slf, Data)
                }
                None => {
                    error!(slf, Error::EofInDoctype);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    eof!()
                }
                c @ Some(_) => {
                    error!(slf, Error::MissingQuoteBeforeDoctypePublicIdentifier);
                    slf.emitter.set_force_quirks();
                    reconsume_in!(slf, c, BogusDoctype)
                }
            }
        )
    });

    define_state!(BeforeDoctypePublicIdentifier, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => cont!(),
                Some(b'"') => {
                    slf.emitter.set_doctype_public_identifier(b"");
                    switch_to!(slf, DoctypePublicIdentifierDoubleQuoted)
                }
                Some(b'\'') => {
                    slf.emitter.set_doctype_public_identifier(b"");
                    switch_to!(slf, DoctypePublicIdentifierSingleQuoted)
                }
                Some(b'>') => {
                    error!(slf, Error::MissingDoctypePublicIdentifier);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    switch_to!(slf, Data)
                }
                None => {
                    error!(slf, Error::EofInDoctype);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    eof!()
                }
                c @ Some(_) => {
                    error!(slf, Error::MissingQuoteBeforeDoctypePublicIdentifier);
                    slf.emitter.set_force_quirks();
                    reconsume_in!(slf, c, BogusDoctype)
                }
            }
        )
    });

    define_state!(DoctypePublicIdentifierDoubleQuoted, slf, {
        fast_read_char!(
            slf,
            match xs {
                Some(b"\"") => {
                    switch_to!(slf, AfterDoctypePublicIdentifier)
                }
                Some(b"\0") => {
                    error!(slf, Error::UnexpectedNullCharacter);
                    slf.emitter
                        .push_doctype_public_identifier("\u{fffd}".as_bytes());
                    cont!()
                }
                Some(b">") => {
                    error!(slf, Error::AbruptDoctypePublicIdentifier);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    switch_to!(slf, Data)
                }
                Some(xs) => {
                    slf.emitter.push_doctype_public_identifier(xs);
                    cont!()
                }
                None => {
                    error!(slf, Error::EofInDoctype);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    eof!()
                }
            }
        )
    });

    define_state!(DoctypePublicIdentifierSingleQuoted, slf, {
        fast_read_char!(
            slf,
            match xs {
                Some(b"'") => {
                    switch_to!(slf, AfterDoctypePublicIdentifier)
                }
                Some(b"\0") => {
                    error!(slf, Error::UnexpectedNullCharacter);
                    slf.emitter
                        .push_doctype_public_identifier("\u{fffd}".as_bytes());
                    cont!()
                }
                Some(b">") => {
                    error!(slf, Error::AbruptDoctypePublicIdentifier);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    switch_to!(slf, Data)
                }
                Some(xs) => {
                    slf.emitter.push_doctype_public_identifier(xs);
                    cont!()
                }
                None => {
                    error!(slf, Error::EofInDoctype);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    eof!()
                }
            }
        )
    });

    define_state!(AfterDoctypePublicIdentifier, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => {
                    switch_to!(slf, BetweenDoctypePublicAndSystemIdentifiers)
                }
                Some(b'>') => {
                    slf.emitter.emit_current_doctype();
                    switch_to!(slf, Data)
                }
                Some(b'"') => {
                    error!(
                        slf,
                        Error::MissingWhitespaceBetweenDoctypePublicAndSystemIdentifiers
                    );
                    slf.emitter.set_doctype_system_identifier(b"");
                    switch_to!(slf, DoctypeSystemIdentifierDoubleQuoted)
                }
                Some(b'\'') => {
                    error!(
                        slf,
                        Error::MissingWhitespaceBetweenDoctypePublicAndSystemIdentifiers
                    );
                    slf.emitter.set_doctype_system_identifier(b"");
                    switch_to!(slf, DoctypeSystemIdentifierSingleQuoted)
                }
                None => {
                    error!(slf, Error::EofInDoctype);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    eof!()
                }
                c @ Some(_) => {
                    error!(slf, Error::MissingQuoteBeforeDoctypeSystemIdentifier);
                    slf.emitter.set_force_quirks();
                    reconsume_in!(slf, c, BogusDoctype)
                }
            }
        )
    });

    define_state!(BetweenDoctypePublicAndSystemIdentifiers, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => cont!(),
                Some(b'>') => {
                    slf.emitter.emit_current_doctype();
                    switch_to!(slf, Data)
                }
                Some(b'"') => {
                    slf.emitter.set_doctype_system_identifier(b"");
                    switch_to!(slf, DoctypeSystemIdentifierDoubleQuoted)
                }
                Some(b'\'') => {
                    slf.emitter.set_doctype_system_identifier(b"");
                    switch_to!(slf, DoctypeSystemIdentifierSingleQuoted)
                }
                None => {
                    error!(slf, Error::EofInDoctype);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    eof!()
                }
                c @ Some(_) => {
                    error!(slf, Error::MissingQuoteBeforeDoctypeSystemIdentifier);
                    slf.emitter.set_force_quirks();
                    reconsume_in!(slf, c, BogusDoctype)
                }
            }
        )
    });

    define_state!(AfterDoctypeSystemKeyword, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => {
                    switch_to!(slf, BeforeDoctypeSystemIdentifier)
                }
                Some(b'"') => {
                    error!(slf, Error::MissingWhitespaceAfterDoctypeSystemKeyword);
                    slf.emitter.set_doctype_system_identifier(b"");
                    switch_to!(slf, DoctypeSystemIdentifierDoubleQuoted)
                }
                Some(b'\'') => {
                    error!(slf, Error::MissingWhitespaceAfterDoctypeSystemKeyword);
                    slf.emitter.set_doctype_system_identifier(b"");
                    switch_to!(slf, DoctypeSystemIdentifierSingleQuoted)
                }
                Some(b'>') => {
                    error!(slf, Error::MissingDoctypeSystemIdentifier);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    switch_to!(slf, Data)
                }
                None => {
                    error!(slf, Error::EofInDoctype);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    eof!()
                }
                c @ Some(_) => {
                    error!(slf, Error::MissingQuoteBeforeDoctypeSystemIdentifier);
                    slf.emitter.set_force_quirks();
                    reconsume_in!(slf, c, BogusDoctype)
                }
            }
        )
    });

    define_state!(BeforeDoctypeSystemIdentifier, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => cont!(),
                Some(b'"') => {
                    slf.emitter.set_doctype_system_identifier(b"");
                    switch_to!(slf, DoctypeSystemIdentifierDoubleQuoted)
                }
                Some(b'\'') => {
                    slf.emitter.set_doctype_system_identifier(b"");
                    switch_to!(slf, DoctypeSystemIdentifierSingleQuoted)
                }
                Some(b'>') => {
                    error!(slf, Error::MissingDoctypeSystemIdentifier);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    switch_to!(slf, Data)
                }
                None => {
                    error!(slf, Error::EofInDoctype);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    eof!()
                }
                c @ Some(_) => {
                    error!(slf, Error::MissingQuoteBeforeDoctypeSystemIdentifier);
                    slf.emitter.set_force_quirks();
                    reconsume_in!(slf, c, BogusDoctype)
                }
            }
        )
    });

    define_state!(DoctypeSystemIdentifierDoubleQuoted, slf, {
        fast_read_char!(
            slf,
            match xs {
                Some(b"\"") => {
                    switch_to!(slf, AfterDoctypeSystemIdentifier)
                }
                Some(b"\0") => {
                    error!(slf, Error::UnexpectedNullCharacter);
                    slf.emitter
                        .push_doctype_system_identifier("\u{fffd}".as_bytes());
                    cont!()
                }
                Some(b">") => {
                    error!(slf, Error::AbruptDoctypeSystemIdentifier);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    switch_to!(slf, Data)
                }
                Some(xs) => {
                    slf.emitter.push_doctype_system_identifier(xs);
                    cont!()
                }
                None => {
                    error!(slf, Error::EofInDoctype);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    eof!()
                }
            }
        )
    });

    define_state!(DoctypeSystemIdentifierSingleQuoted, slf, {
        fast_read_char!(
            slf,
            match xs {
                Some(b"\'") => {
                    switch_to!(slf, AfterDoctypeSystemIdentifier)
                }
                Some(b"\0") => {
                    error!(slf, Error::UnexpectedNullCharacter);
                    slf.emitter
                        .push_doctype_system_identifier("\u{fffd}".as_bytes());
                    cont!()
                }
                Some(b">") => {
                    error!(slf, Error::AbruptDoctypeSystemIdentifier);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    switch_to!(slf, Data)
                }
                Some(xs) => {
                    slf.emitter.push_doctype_system_identifier(xs);
                    cont!()
                }
                None => {
                    error!(slf, Error::EofInDoctype);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    eof!()
                }
            }
        )
    });

    define_state!(AfterDoctypeSystemIdentifier, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => cont!(),
                Some(b'>') => {
                    slf.emitter.emit_current_doctype();
                    switch_to!(slf, Data)
                }
                None => {
                    error!(slf, Error::EofInDoctype);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    eof!()
                }
                c @ Some(_) => {
                    error!(slf, Error::UnexpectedCharacterAfterDoctypeSystemIdentifier);
                    reconsume_in!(slf, c, BogusDoctype)
                }
            }
        )
    });

    define_state!(BogusDoctype, slf, {
        fast_read_char!(
            slf,
            match xs {
                Some(b">") => {
                    slf.emitter.emit_current_doctype();
                    switch_to!(slf, Data)
                }
                Some(b"\0") => {
                    error!(slf, Error::UnexpectedNullCharacter);
                    cont!()
                }
                Some(_xs) => {
                    cont!()
                }
                None => {
                    slf.emitter.emit_current_doctype();
                    eof!()
                }
            }
        )
    });

    define_state!(CdataSection, slf, {
        fast_read_char!(
            slf,
            match xs {
                Some(b"]") => {
                    switch_to!(slf, CdataSectionBracket)
                }
                Some(xs) => {
                    slf.emitter.emit_string(xs);
                    cont!()
                }
                None => {
                    error!(slf, Error::EofInCdata);
                    eof!()
                }
            }
        )
    });

    define_state!(CdataSectionBracket, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b']') => {
                    switch_to!(slf, CdataSectionEnd)
                }
                c => {
                    slf.emitter.emit_string(b"]");
                    reconsume_in!(slf, c, CdataSection)
                }
            }
        )
    });

    define_state!(CdataSectionEnd, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(b']') => {
                    slf.emitter.emit_string(b"]");
                    cont!()
                }
                Some(b'>') => {
                    switch_to!(slf, Data)
                }
                c => {
                    slf.emitter.emit_string(b"]]");
                    reconsume_in!(slf, c, CdataSection)
                }
            }
        )
    });

    define_state!(CharacterReference, slf, {
        slf.machine_helper.temporary_buffer.clear();
        slf.machine_helper.temporary_buffer.push(b'&');

        slow_read_byte!(
            slf,
            match c {
                Some(x) if x.is_ascii_alphanumeric() => {
                    reconsume_in!(slf, Some(x), NamedCharacterReference)
                }
                Some(b'#') => {
                    slf.machine_helper.temporary_buffer.push(b'#');
                    switch_to!(slf, NumericCharacterReference)
                }
                c => {
                    // since c is not part of the flushed characters, we temporarily undo the
                    // move_position done by slow_read_byte, then redo it, then let reconsume
                    // re-undo the redo.
                    //
                    // 1. move_position(-1) // revert slow_read_byte
                    // 2. flush code points
                    // 3. move_position(1)
                    // 4. reconsume: move_position(-1)
                    slf.emitter.move_position(-1);
                    slf.machine_helper
                        .flush_code_points_consumed_as_character_reference(&mut slf.emitter);
                    slf.emitter.move_position(1);
                    reconsume_in_return_state!(slf, c)
                }
            }
        )
    });

    define_state!(NamedCharacterReference, slf, {
        let c = read_byte!(slf)?;

        let char_ref = match c {
            Some(x) => try_read_character_reference(x as char, |x| slf.try_read_string(x, true))?
                .map(|char_ref| (x, char_ref)),

            None => None,
        };

        if let Some((x, char_ref)) = char_ref {
            let char_ref_name_last_character = char_ref.name.chars().last();
            let next_character = read_byte!(slf)?;

            if !slf.machine_helper.is_consumed_as_part_of_an_attribute()
                || char_ref_name_last_character == Some(';')
                || !matches!(next_character, Some(x) if x == b'=' || x.is_ascii_alphanumeric())
            {
                if char_ref_name_last_character != Some(';') {
                    error!(slf, Error::MissingSemicolonAfterCharacterReference);
                }

                slf.machine_helper.temporary_buffer.clear();
                slf.machine_helper
                    .temporary_buffer
                    .extend(char_ref.characters.as_bytes());
            } else {
                slf.machine_helper.temporary_buffer.extend(&[x]);
                slf.machine_helper
                    .temporary_buffer
                    .extend(char_ref.name.as_bytes());
            }

            slf.emitter.move_position(-1);
            slf.machine_helper
                .flush_code_points_consumed_as_character_reference(&mut slf.emitter);
            slf.emitter.move_position(1);
            reconsume_in_return_state!(slf, next_character)
        } else {
            slf.emitter.move_position(-1);
            slf.machine_helper
                .flush_code_points_consumed_as_character_reference(&mut slf.emitter);
            slf.emitter.move_position(1);
            reconsume_in!(slf, c, AmbiguousAmpersand)
        }
    });

    define_state!(AmbiguousAmpersand, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(x) if x.is_ascii_alphanumeric() => {
                    if slf.machine_helper.is_consumed_as_part_of_an_attribute() {
                        slf.emitter.push_attribute_value(&[x]);
                    } else {
                        slf.emitter.emit_string(&[x]);
                    }

                    cont!()
                }
                c @ Some(b';') => {
                    error!(slf, Error::UnknownNamedCharacterReference);
                    reconsume_in_return_state!(slf, c)
                }
                c => {
                    reconsume_in_return_state!(slf, c)
                }
            }
        )
    });

    define_state!(NumericCharacterReference, slf, {
        slf.machine_helper.character_reference_code = 0;

        slow_read_byte!(
            slf,
            match c {
                Some(x @ (b'x' | b'X')) => {
                    slf.machine_helper.temporary_buffer.push(x);
                    switch_to!(slf, HexadecimalCharacterReferenceStart)
                }
                Some(x @ b'0'..=b'9') => {
                    reconsume_in!(slf, Some(x), DecimalCharacterReference)
                }
                c => {
                    error!(slf, Error::AbsenceOfDigitsInNumericCharacterReference);
                    slf.emitter.move_position(-1);
                    slf.machine_helper
                        .flush_code_points_consumed_as_character_reference(&mut slf.emitter);
                    slf.emitter.move_position(1);
                    reconsume_in_return_state!(slf, c)
                }
            }
        )
    });

    define_state!(HexadecimalCharacterReferenceStart, slf, {
        slow_read_byte!(
            slf,
            match c {
                c @ Some(b'0'..=b'9' | b'A'..=b'F' | b'a'..=b'f') => {
                    reconsume_in!(slf, c, HexadecimalCharacterReference)
                }
                c => {
                    error!(slf, Error::AbsenceOfDigitsInNumericCharacterReference);
                    slf.machine_helper
                        .flush_code_points_consumed_as_character_reference(&mut slf.emitter);
                    reconsume_in_return_state!(slf, c)
                }
            }
        )
    });

    define_state!(HexadecimalCharacterReference, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(x @ b'0'..=b'9') => {
                    mutate_character_reference!(slf, *16 + x - 0x0030);
                    cont!()
                }
                Some(x @ b'A'..=b'F') => {
                    mutate_character_reference!(slf, *16 + x - 0x0037);
                    cont!()
                }
                Some(x @ b'a'..=b'f') => {
                    mutate_character_reference!(slf, *16 + x - 0x0057);
                    cont!()
                }
                Some(b';') => {
                    switch_to!(slf, NumericCharacterReferenceEnd)
                }
                c => {
                    error!(slf, Error::MissingSemicolonAfterCharacterReference);
                    reconsume_in!(slf, c, NumericCharacterReferenceEnd)
                }
            }
        )
    });

    define_state!(DecimalCharacterReference, slf, {
        slow_read_byte!(
            slf,
            match c {
                Some(x @ b'0'..=b'9') => {
                    mutate_character_reference!(slf, *10 + x - 0x0030);
                    cont!()
                }
                Some(b';') => {
                    switch_to!(slf, NumericCharacterReferenceEnd)
                }
                c => {
                    error!(slf, Error::MissingSemicolonAfterCharacterReference);
                    reconsume_in!(slf, c, NumericCharacterReferenceEnd)
                }
            }
        )
    });

    define_state!(NumericCharacterReferenceEnd, slf, {
        match slf.machine_helper.character_reference_code {
            0x00 => {
                error!(slf, Error::NullCharacterReference);
                slf.machine_helper.character_reference_code = 0xfffd;
            }
            0x0011_0000.. => {
                error!(slf, Error::CharacterReferenceOutsideUnicodeRange);
                slf.machine_helper.character_reference_code = 0xfffd;
            }
            surrogate_pat!() => {
                error!(slf, Error::SurrogateCharacterReference);
                slf.machine_helper.character_reference_code = 0xfffd;
            }
            // noncharacter
            noncharacter_pat!() => {
                error!(slf, Error::NoncharacterCharacterReference);
            }
            // 0x000d, or a control that is not whitespace
            x @ (0x000d | 0x0d | 0x0000..=0x001f | 0x007f..=0x009f)
                if !matches!(x, 0x0009 | 0x000a | 0x000c | 0x0020) =>
            {
                error!(slf, Error::ControlCharacterReference);
                slf.machine_helper.character_reference_code = match x {
                    0x80 => 0x20AC, // EURO SIGN (€)
                    0x82 => 0x201A, // SINGLE LOW-9 QUOTATION MARK (‚)
                    0x83 => 0x0192, // LATIN SMALL LETTER F WITH HOOK (ƒ)
                    0x84 => 0x201E, // DOUBLE LOW-9 QUOTATION MARK („)
                    0x85 => 0x2026, // HORIZONTAL ELLIPSIS (…)
                    0x86 => 0x2020, // DAGGER (†)
                    0x87 => 0x2021, // DOUBLE DAGGER (‡)
                    0x88 => 0x02C6, // MODIFIER LETTER CIRCUMFLEX ACCENT (ˆ)
                    0x89 => 0x2030, // PER MILLE SIGN (‰)
                    0x8A => 0x0160, // LATIN CAPITAL LETTER S WITH CARON (Š)
                    0x8B => 0x2039, // SINGLE LEFT-POINTING ANGLE QUOTATION MARK (‹)
                    0x8C => 0x0152, // LATIN CAPITAL LIGATURE OE (Œ)
                    0x8E => 0x017D, // LATIN CAPITAL LETTER Z WITH CARON (Ž)
                    0x91 => 0x2018, // LEFT SINGLE QUOTATION MARK (‘)
                    0x92 => 0x2019, // RIGHT SINGLE QUOTATION MARK (’)
                    0x93 => 0x201C, // LEFT DOUBLE QUOTATION MARK (“)
                    0x94 => 0x201D, // RIGHT DOUBLE QUOTATION MARK (”)
                    0x95 => 0x2022, // BULLET (•)
                    0x96 => 0x2013, // EN DASH (–)
                    0x97 => 0x2014, // EM DASH (—)
                    0x98 => 0x02DC, // SMALL TILDE (˜)
                    0x99 => 0x2122, // TRADE MARK SIGN (™)
                    0x9A => 0x0161, // LATIN SMALL LETTER S WITH CARON (š)
                    0x9B => 0x203A, // SINGLE RIGHT-POINTING ANGLE QUOTATION MARK (›)
                    0x9C => 0x0153, // LATIN SMALL LIGATURE OE (œ)
                    0x9E => 0x017E, // LATIN SMALL LETTER Z WITH CARON (ž)
                    0x9F => 0x0178, // LATIN CAPITAL LETTER Y WITH DIAERESIS (Ÿ)
                    _ => slf.machine_helper.character_reference_code,
                };
            }
            _ => (),
        }

        slf.machine_helper.temporary_buffer.clear();
        slf.machine_helper.temporary_buffer.extend(
            ctostr!(std::char::from_u32(slf.machine_helper.character_reference_code).unwrap())
                .as_bytes(),
        );
        slf.machine_helper
            .flush_code_points_consumed_as_character_reference(&mut slf.emitter);
        exit_state!(slf)
    });
}
