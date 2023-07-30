use crate::entities::try_read_character_reference;
use crate::machine_helper::{
    cont, emit_current_tag_and_switch_to, enter_state, eof, error, error_immediate, exit_state,
    mutate_character_reference, read_byte, reconsume_in, switch_to,
};
use crate::read_helper::{fast_read_char, slow_read_byte};
use crate::state::MachineState as State;
use crate::utils::{ctostr, noncharacter_pat, surrogate_pat, with_lowercase_str, ControlToken};
use crate::{Emitter, Error, Reader, Tokenizer};

macro_rules! define_state {
    ($state:ident, $slf:ident, $($body:tt)*) => {
        #[allow(non_snake_case)]
        mod $state {
            use super::*;

            pub(crate) fn run<R: Reader, E: Emitter>($slf: &mut Tokenizer<R, E>) -> Result<ControlToken, R::Error> {
                $($body)*
            }
        }
    };
}

macro_rules! call_state {
    ($state:ident, $slf:expr) => {
        $state::run($slf)
    };
}

define_state!(Data, slf, {
    fast_read_char!(slf, match xs {
        Some(b"&") => {
            enter_state!(slf, State::CharacterReference)
        }
        Some(b"<") => {
            switch_to!(slf, State::TagOpen)
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
    })
});

define_state!(RcData, slf, {
    fast_read_char!(
        slf,
        match xs {
            Some(b"&") => {
                enter_state!(slf, State::CharacterReference)
            }
            Some(b"<") => {
                switch_to!(slf, State::RcDataLessThanSign)
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
                switch_to!(slf, State::RawTextLessThanSign)
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

// Note: This is not implemented as a method on Tokenizer because there's fields on Tokenizer that
// should not be available in this method, such as Tokenizer.to_reconsume or the Reader instance
pub(crate) fn consume<R: Reader, E: Emitter>(
    slf: &mut Tokenizer<R, E>,
) -> Result<ControlToken, R::Error> {
    match slf.machine_helper.state() {
        State::ScriptData => fast_read_char!(
            slf,
            match xs {
                Some(b"<") => {
                    switch_to!(slf, State::ScriptDataLessThanSign)
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
        ),
        State::PlainText => fast_read_char!(
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
        ),
        State::TagOpen => slow_read_byte!(
            slf,
            match c {
                Some(b'!') => {
                    switch_to!(slf, State::MarkupDeclarationOpen)
                }
                Some(b'/') => {
                    switch_to!(slf, State::EndTagOpen)
                }
                Some(x) if x.is_ascii_alphabetic() => {
                    slf.emitter.init_start_tag();
                    reconsume_in!(slf, Some(x), State::TagName)
                }
                c @ Some(b'?') => {
                    error!(slf, Error::UnexpectedQuestionMarkInsteadOfTagName);
                    slf.emitter.init_comment();
                    reconsume_in!(slf, c, State::BogusComment)
                }
                None => {
                    error!(slf, Error::EofBeforeTagName);
                    slf.emitter.emit_string(b"<");
                    eof!()
                }
                c @ Some(_) => {
                    error!(slf, Error::InvalidFirstCharacterOfTagName);
                    slf.emitter.emit_string(b"<");
                    reconsume_in!(slf, c, State::Data)
                }
            }
        ),
        State::EndTagOpen => slow_read_byte!(
            slf,
            match c {
                Some(x) if x.is_ascii_alphabetic() => {
                    slf.emitter.init_end_tag();
                    reconsume_in!(slf, Some(x), State::TagName)
                }
                Some(b'>') => {
                    error!(slf, Error::MissingEndTagName);
                    switch_to!(slf, State::Data)
                }
                None => {
                    error!(slf, Error::EofBeforeTagName);
                    slf.emitter.emit_string(b"</");
                    eof!()
                }
                Some(x) => {
                    error!(slf, Error::InvalidFirstCharacterOfTagName);
                    slf.emitter.init_comment();
                    reconsume_in!(slf, Some(x), State::BogusComment)
                }
            }
        ),
        State::TagName => fast_read_char!(
            slf,
            match xs {
                Some(b"\t" | b"\x0A" | b"\x0C" | b" ") => {
                    switch_to!(slf, State::BeforeAttributeName)
                }
                Some(b"/") => {
                    switch_to!(slf, State::SelfClosingStartTag)
                }
                Some(b">") => {
                    emit_current_tag_and_switch_to!(slf, State::Data)
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
        ),
        State::RcDataLessThanSign => slow_read_byte!(
            slf,
            match c {
                Some(b'/') => {
                    slf.machine_helper.temporary_buffer.clear();
                    switch_to!(slf, State::RcDataEndTagOpen)
                }
                c => {
                    slf.emitter.emit_string(b"<");
                    reconsume_in!(slf, c, State::RcData)
                }
            }
        ),
        State::RcDataEndTagOpen => slow_read_byte!(
            slf,
            match c {
                Some(x) if x.is_ascii_alphabetic() => {
                    slf.emitter.init_end_tag();
                    reconsume_in!(slf, Some(x), State::RcDataEndTagName)
                }
                c => {
                    slf.emitter.emit_string(b"</");
                    reconsume_in!(slf, c, State::RcData)
                }
            }
        ),
        State::RcDataEndTagName => slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ')
                    if slf.emitter.current_is_appropriate_end_tag_token() =>
                {
                    switch_to!(slf, State::BeforeAttributeName)
                }
                Some(b'/') if slf.emitter.current_is_appropriate_end_tag_token() => {
                    switch_to!(slf, State::SelfClosingStartTag)
                }
                Some(b'>') if slf.emitter.current_is_appropriate_end_tag_token() => {
                    emit_current_tag_and_switch_to!(slf, State::Data)
                }
                Some(x) if x.is_ascii_alphabetic() => {
                    slf.emitter.push_tag_name(&[x.to_ascii_lowercase()]);
                    slf.machine_helper.temporary_buffer.push(x);
                    cont!()
                }
                c => {
                    slf.emitter.emit_string(b"</");
                    slf.machine_helper.flush_buffer_characters(&mut slf.emitter);
                    reconsume_in!(slf, c, State::RcData)
                }
            }
        ),
        State::RawTextLessThanSign => slow_read_byte!(
            slf,
            match c {
                Some(b'/') => {
                    slf.machine_helper.temporary_buffer.clear();
                    switch_to!(slf, State::RawTextEndTagOpen)
                }
                c => {
                    slf.emitter.emit_string(b"<");
                    reconsume_in!(slf, c, State::RawText)
                }
            }
        ),
        State::RawTextEndTagOpen => slow_read_byte!(
            slf,
            match c {
                Some(x) if x.is_ascii_alphabetic() => {
                    slf.emitter.init_end_tag();
                    reconsume_in!(slf, Some(x), State::RawTextEndTagName)
                }
                c => {
                    slf.emitter.emit_string(b"</");
                    reconsume_in!(slf, c, State::RawText)
                }
            }
        ),
        State::RawTextEndTagName => slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ')
                    if slf.emitter.current_is_appropriate_end_tag_token() =>
                {
                    switch_to!(slf, State::BeforeAttributeName)
                }
                Some(b'/') if slf.emitter.current_is_appropriate_end_tag_token() => {
                    switch_to!(slf, State::SelfClosingStartTag)
                }
                Some(b'>') if slf.emitter.current_is_appropriate_end_tag_token() => {
                    emit_current_tag_and_switch_to!(slf, State::Data)
                }
                Some(x) if x.is_ascii_alphabetic() => {
                    slf.emitter.push_tag_name(&[x.to_ascii_lowercase()]);
                    slf.machine_helper.temporary_buffer.push(x);
                    cont!()
                }
                c => {
                    slf.emitter.emit_string(b"</");
                    slf.machine_helper.flush_buffer_characters(&mut slf.emitter);
                    reconsume_in!(slf, c, State::RawText)
                }
            }
        ),
        State::ScriptDataLessThanSign => slow_read_byte!(
            slf,
            match c {
                Some(b'/') => {
                    slf.machine_helper.temporary_buffer.clear();
                    switch_to!(slf, State::ScriptDataEndTagOpen)
                }
                Some(b'!') => {
                    slf.emitter.emit_string(b"<!");
                    switch_to!(slf, State::ScriptDataEscapeStart)
                }
                c => {
                    slf.emitter.emit_string(b"<");
                    reconsume_in!(slf, c, State::ScriptData)
                }
            }
        ),
        State::ScriptDataEndTagOpen => slow_read_byte!(
            slf,
            match c {
                Some(x) if x.is_ascii_alphabetic() => {
                    slf.emitter.init_end_tag();
                    reconsume_in!(slf, Some(x), State::ScriptDataEndTagName)
                }
                c => {
                    slf.emitter.emit_string(b"</");
                    reconsume_in!(slf, c, State::ScriptData)
                }
            }
        ),
        State::ScriptDataEndTagName => slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ')
                    if slf.emitter.current_is_appropriate_end_tag_token() =>
                {
                    switch_to!(slf, State::BeforeAttributeName)
                }
                Some(b'/') if slf.emitter.current_is_appropriate_end_tag_token() => {
                    switch_to!(slf, State::SelfClosingStartTag)
                }
                Some(b'>') if slf.emitter.current_is_appropriate_end_tag_token() => {
                    emit_current_tag_and_switch_to!(slf, State::Data)
                }
                Some(x) if x.is_ascii_alphabetic() => {
                    slf.emitter.push_tag_name(&[x.to_ascii_lowercase()]);
                    slf.machine_helper
                        .temporary_buffer
                        .push(x.to_ascii_lowercase());
                    cont!()
                }
                c => {
                    slf.emitter.emit_string(b"</");
                    slf.machine_helper.flush_buffer_characters(&mut slf.emitter);
                    reconsume_in!(slf, c, State::Data)
                }
            }
        ),
        State::ScriptDataEscapeStart => slow_read_byte!(
            slf,
            match c {
                Some(b'-') => {
                    slf.emitter.emit_string(b"-");
                    switch_to!(slf, State::ScriptDataEscapeStartDash)
                }
                c => {
                    reconsume_in!(slf, c, State::ScriptData)
                }
            }
        ),
        State::ScriptDataEscapeStartDash => slow_read_byte!(
            slf,
            match c {
                Some(b'-') => {
                    slf.emitter.emit_string(b"-");
                    switch_to!(slf, State::ScriptDataEscapedDashDash)
                }
                c => {
                    reconsume_in!(slf, c, State::ScriptData)
                }
            }
        ),
        State::ScriptDataEscaped => fast_read_char!(
            slf,
            match xs {
                Some(b"-") => {
                    slf.emitter.emit_string(b"-");
                    switch_to!(slf, State::ScriptDataEscapedDash)
                }
                Some(b"<") => {
                    switch_to!(slf, State::ScriptDataEscapedLessThanSign)
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
        ),
        State::ScriptDataEscapedDash => slow_read_byte!(
            slf,
            match c {
                Some(b'-') => {
                    slf.emitter.emit_string(b"-");
                    switch_to!(slf, State::ScriptDataEscapedDashDash)
                }
                Some(b'<') => {
                    switch_to!(slf, State::ScriptDataEscapedLessThanSign)
                }
                Some(b'\0') => {
                    error!(slf, Error::UnexpectedNullCharacter);
                    slf.emitter.emit_string("\u{fffd}".as_bytes());
                    switch_to!(slf, State::ScriptDataEscaped)
                }
                Some(x) => {
                    slf.emitter.emit_string(&[x]);
                    switch_to!(slf, State::ScriptDataEscaped)
                }
                None => {
                    error!(slf, Error::EofInScriptHtmlCommentLikeText);
                    eof!()
                }
            }
        ),
        State::ScriptDataEscapedDashDash => slow_read_byte!(
            slf,
            match c {
                Some(b'-') => {
                    slf.emitter.emit_string(b"-");
                    cont!()
                }
                Some(b'<') => {
                    switch_to!(slf, State::ScriptDataEscapedLessThanSign)
                }
                Some(b'>') => {
                    slf.emitter.emit_string(b">");
                    switch_to!(slf, State::ScriptData)
                }
                Some(b'\0') => {
                    error!(slf, Error::UnexpectedNullCharacter);
                    slf.emitter.emit_string("\u{fffd}".as_bytes());
                    switch_to!(slf, State::ScriptDataEscaped)
                }
                Some(x) => {
                    slf.emitter.emit_string(&[x]);
                    switch_to!(slf, State::ScriptDataEscaped)
                }
                None => {
                    error!(slf, Error::EofInScriptHtmlCommentLikeText);
                    eof!()
                }
            }
        ),
        State::ScriptDataEscapedLessThanSign => slow_read_byte!(
            slf,
            match c {
                Some(b'/') => {
                    slf.machine_helper.temporary_buffer.clear();
                    switch_to!(slf, State::ScriptDataEscapedEndTagOpen)
                }
                Some(x) if x.is_ascii_alphabetic() => {
                    slf.machine_helper.temporary_buffer.clear();
                    slf.emitter.emit_string(b"<");
                    reconsume_in!(slf, Some(x), State::ScriptDataDoubleEscapeStart)
                }
                c => {
                    slf.emitter.emit_string(b"<");
                    reconsume_in!(slf, c, State::ScriptDataEscaped)
                }
            }
        ),
        State::ScriptDataEscapedEndTagOpen => slow_read_byte!(
            slf,
            match c {
                Some(x) if x.is_ascii_alphabetic() => {
                    slf.emitter.init_end_tag();
                    reconsume_in!(slf, Some(x), State::ScriptDataEscapedEndTagName)
                }
                c => {
                    slf.emitter.emit_string(b"</");
                    reconsume_in!(slf, c, State::ScriptDataEscaped)
                }
            }
        ),
        State::ScriptDataEscapedEndTagName => slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ')
                    if slf.emitter.current_is_appropriate_end_tag_token() =>
                {
                    switch_to!(slf, State::BeforeAttributeName)
                }
                Some(b'/') if slf.emitter.current_is_appropriate_end_tag_token() => {
                    switch_to!(slf, State::SelfClosingStartTag)
                }
                Some(b'>') if slf.emitter.current_is_appropriate_end_tag_token() => {
                    emit_current_tag_and_switch_to!(slf, State::Data)
                }
                Some(x) if x.is_ascii_alphabetic() => {
                    slf.emitter.push_tag_name(&[x.to_ascii_lowercase()]);
                    slf.machine_helper.temporary_buffer.extend(&[x]);
                    cont!()
                }
                c => {
                    slf.emitter.emit_string(b"</");
                    slf.machine_helper.flush_buffer_characters(&mut slf.emitter);
                    reconsume_in!(slf, c, State::ScriptDataEscaped)
                }
            }
        ),
        State::ScriptDataDoubleEscapeStart => slow_read_byte!(
            slf,
            match c {
                Some(x @ (b'\t' | b'\x0A' | b'\x0C' | b' ' | b'/' | b'>')) => {
                    slf.emitter.emit_string(&[x]);
                    if slf.machine_helper.temporary_buffer == b"script" {
                        switch_to!(slf, State::ScriptDataDoubleEscaped)
                    } else {
                        switch_to!(slf, State::ScriptDataEscaped)
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
                    reconsume_in!(slf, c, State::ScriptDataEscaped)
                }
            }
        ),
        State::ScriptDataDoubleEscaped => fast_read_char!(
            slf,
            match xs {
                Some(b"-") => {
                    slf.emitter.emit_string(b"-");
                    switch_to!(slf, State::ScriptDataDoubleEscapedDash)
                }
                Some(b"<") => {
                    slf.emitter.emit_string(b"<");
                    switch_to!(slf, State::ScriptDataDoubleEscapedLessThanSign)
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
        ),
        State::ScriptDataDoubleEscapedDash => slow_read_byte!(
            slf,
            match c {
                Some(b'-') => {
                    slf.emitter.emit_string(b"-");
                    switch_to!(slf, State::ScriptDataDoubleEscapedDashDash)
                }
                Some(b'<') => {
                    slf.emitter.emit_string(b"<");
                    switch_to!(slf, State::ScriptDataDoubleEscapedLessThanSign)
                }
                Some(b'\0') => {
                    error!(slf, Error::UnexpectedNullCharacter);
                    slf.emitter.emit_string("\u{fffd}".as_bytes());
                    switch_to!(slf, State::ScriptDataDoubleEscaped)
                }
                Some(x) => {
                    slf.emitter.emit_string(&[x]);
                    switch_to!(slf, State::ScriptDataDoubleEscaped)
                }
                None => {
                    error!(slf, Error::EofInScriptHtmlCommentLikeText);
                    eof!()
                }
            }
        ),
        State::ScriptDataDoubleEscapedDashDash => slow_read_byte!(
            slf,
            match c {
                Some(b'-') => {
                    slf.emitter.emit_string(b"-");
                    cont!()
                }
                Some(b'<') => {
                    slf.emitter.emit_string(b"<");
                    switch_to!(slf, State::ScriptDataDoubleEscapedLessThanSign)
                }
                Some(b'>') => {
                    slf.emitter.emit_string(b">");
                    switch_to!(slf, State::ScriptData)
                }
                Some(b'\0') => {
                    error!(slf, Error::UnexpectedNullCharacter);
                    slf.emitter.emit_string("\u{fffd}".as_bytes());
                    switch_to!(slf, State::ScriptDataDoubleEscaped)
                }
                Some(x) => {
                    slf.emitter.emit_string(&[x]);
                    switch_to!(slf, State::ScriptDataDoubleEscaped)
                }
                None => {
                    error!(slf, Error::EofInScriptHtmlCommentLikeText);
                    eof!()
                }
            }
        ),
        State::ScriptDataDoubleEscapedLessThanSign => slow_read_byte!(
            slf,
            match c {
                Some(b'/') => {
                    slf.machine_helper.temporary_buffer.clear();
                    slf.emitter.emit_string(b"/");
                    switch_to!(slf, State::ScriptDataDoubleEscapeEnd)
                }
                c => {
                    reconsume_in!(slf, c, State::ScriptDataDoubleEscaped)
                }
            }
        ),
        State::ScriptDataDoubleEscapeEnd => slow_read_byte!(
            slf,
            match c {
                Some(x @ (b'\t' | b'\x0A' | b'\x0C' | b' ' | b'/' | b'>')) => {
                    slf.emitter.emit_string(&[x]);

                    if slf.machine_helper.temporary_buffer == b"script" {
                        switch_to!(slf, State::ScriptDataEscaped)
                    } else {
                        switch_to!(slf, State::ScriptDataDoubleEscaped)
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
                    reconsume_in!(slf, c, State::ScriptDataDoubleEscaped)
                }
            }
        ),
        State::BeforeAttributeName => slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => cont!(),
                c @ (Some(b'/' | b'>') | None) => {
                    reconsume_in!(slf, c, State::AfterAttributeName)
                }
                Some(b'=') => {
                    error!(slf, Error::UnexpectedEqualsSignBeforeAttributeName);
                    slf.emitter.init_attribute();
                    slf.emitter.push_attribute_name("=".as_bytes());
                    switch_to!(slf, State::AttributeName)
                }
                Some(x) => {
                    slf.emitter.init_attribute();
                    reconsume_in!(slf, Some(x), State::AttributeName)
                }
            }
        ),
        State::AttributeName => fast_read_char!(
            slf,
            match xs {
                Some(b"\t" | b"\x0A" | b"\x0C" | b" " | b"/" | b">") => {
                    reconsume_in!(slf, Some(xs.unwrap()[0]), State::AfterAttributeName)
                }
                Some(b"=") => {
                    switch_to!(slf, State::BeforeAttributeValue)
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
                    reconsume_in!(slf, None, State::AfterAttributeName)
                }
            }
        ),
        State::AfterAttributeName => slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => cont!(),
                Some(b'/') => {
                    switch_to!(slf, State::SelfClosingStartTag)
                }
                Some(b'=') => {
                    switch_to!(slf, State::BeforeAttributeValue)
                }
                Some(b'>') => {
                    emit_current_tag_and_switch_to!(slf, State::Data)
                }
                None => {
                    error!(slf, Error::EofInTag);
                    eof!()
                }
                Some(x) => {
                    slf.emitter.init_attribute();
                    reconsume_in!(slf, Some(x), State::AttributeName)
                }
            }
        ),
        State::BeforeAttributeValue => slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => cont!(),
                Some(b'"') => {
                    switch_to!(slf, State::AttributeValueDoubleQuoted)
                }
                Some(b'\'') => {
                    switch_to!(slf, State::AttributeValueSingleQuoted)
                }
                Some(b'>') => {
                    error!(slf, Error::MissingAttributeValue);
                    emit_current_tag_and_switch_to!(slf, State::Data)
                }
                c => {
                    reconsume_in!(slf, c, State::AttributeValueUnquoted)
                }
            }
        ),
        State::AttributeValueDoubleQuoted => fast_read_char!(
            slf,
            match xs {
                Some(b"\"") => {
                    switch_to!(slf, State::AfterAttributeValueQuoted)
                }
                Some(b"&") => {
                    enter_state!(slf, State::CharacterReference)
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
        ),
        State::AttributeValueSingleQuoted => fast_read_char!(
            slf,
            match xs {
                Some(b"'") => {
                    switch_to!(slf, State::AfterAttributeValueQuoted)
                }
                Some(b"&") => {
                    enter_state!(slf, State::CharacterReference)
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
        ),
        State::AttributeValueUnquoted => fast_read_char!(
            slf,
            match xs {
                Some(b"\t" | b"\x0A" | b"\x0C" | b" ") => {
                    switch_to!(slf, State::BeforeAttributeName)
                }
                Some(b"&") => {
                    enter_state!(slf, State::CharacterReference)
                }
                Some(b">") => {
                    emit_current_tag_and_switch_to!(slf, State::Data)
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
        ),
        State::AfterAttributeValueQuoted => slow_read_byte!(
            slf,
            match c {
                c @ (Some(b'\t' | b'\x0A' | b'\x0C' | b' ' | b'/' | b'>') | None) => {
                    reconsume_in!(slf, c, State::BeforeAttributeName)
                }
                c => {
                    error!(slf, Error::MissingWhitespaceBetweenAttributes);
                    reconsume_in!(slf, c, State::BeforeAttributeName)
                }
            }
        ),
        State::SelfClosingStartTag => slow_read_byte!(
            slf,
            match c {
                Some(b'>') => {
                    slf.emitter.set_self_closing();
                    emit_current_tag_and_switch_to!(slf, State::Data)
                }
                None => {
                    error!(slf, Error::EofInTag);
                    eof!()
                }
                Some(x) => {
                    error_immediate!(slf, Error::UnexpectedSolidusInTag);
                    reconsume_in!(slf, Some(x), State::BeforeAttributeName)
                }
            }
        ),
        State::BogusComment => fast_read_char!(
            slf,
            match xs {
                Some(b">") => {
                    slf.emitter.emit_current_comment();
                    switch_to!(slf, State::Data)
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
        ),
        State::MarkupDeclarationOpen => slow_read_byte!(
            slf,
            match c {
                Some(b'-') if slf.reader.try_read_string(&mut slf.validator, "-", true)? => {
                    slf.emitter.init_comment();
                    switch_to!(slf, State::CommentStart)
                }
                Some(b'd' | b'D')
                    if slf
                        .reader
                        .try_read_string(&mut slf.validator, "octype", false)? =>
                {
                    switch_to!(slf, State::Doctype)
                }
                Some(b'[')
                    if slf
                        .reader
                        .try_read_string(&mut slf.validator, "CDATA[", true)? =>
                {
                    if slf
                        .emitter
                        .adjusted_current_node_present_but_not_in_html_namespace()
                    {
                        switch_to!(slf, State::CdataSection)
                    } else {
                        error!(slf, Error::CdataInHtmlContent);

                        slf.emitter.init_comment();
                        slf.emitter.push_comment(b"[CDATA[");
                        switch_to!(slf, State::BogusComment)
                    }
                }
                c => {
                    error!(slf, Error::IncorrectlyOpenedComment);
                    slf.emitter.init_comment();
                    reconsume_in!(slf, c, State::BogusComment)
                }
            }
        ),
        State::CommentStart => slow_read_byte!(
            slf,
            match c {
                Some(b'-') => {
                    switch_to!(slf, State::CommentStartDash)
                }
                Some(b'>') => {
                    error!(slf, Error::AbruptClosingOfEmptyComment);
                    slf.emitter.emit_current_comment();
                    switch_to!(slf, State::Data)
                }
                c => {
                    reconsume_in!(slf, c, State::Comment)
                }
            }
        ),
        State::CommentStartDash => slow_read_byte!(
            slf,
            match c {
                Some(b'-') => {
                    switch_to!(slf, State::CommentEnd)
                }
                Some(b'>') => {
                    error!(slf, Error::AbruptClosingOfEmptyComment);
                    slf.emitter.emit_current_comment();
                    switch_to!(slf, State::Data)
                }
                None => {
                    error!(slf, Error::EofInComment);
                    slf.emitter.emit_current_comment();
                    eof!()
                }
                c @ Some(_) => {
                    slf.emitter.push_comment(b"-");
                    reconsume_in!(slf, c, State::Comment)
                }
            }
        ),
        State::Comment => fast_read_char!(
            slf,
            match xs {
                Some(b"<") => {
                    slf.emitter.push_comment(b"<");
                    switch_to!(slf, State::CommentLessThanSign)
                }
                Some(b"-") => {
                    switch_to!(slf, State::CommentEndDash)
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
        ),
        State::CommentLessThanSign => slow_read_byte!(
            slf,
            match c {
                Some(b'!') => {
                    slf.emitter.push_comment(b"!");
                    switch_to!(slf, State::CommentLessThanSignBang)
                }
                Some(b'<') => {
                    slf.emitter.push_comment(b"<");
                    cont!()
                }
                c => {
                    reconsume_in!(slf, c, State::Comment)
                }
            }
        ),
        State::CommentLessThanSignBang => slow_read_byte!(
            slf,
            match c {
                Some(b'-') => {
                    switch_to!(slf, State::CommentLessThanSignBangDash)
                }
                c => {
                    reconsume_in!(slf, c, State::Comment)
                }
            }
        ),
        State::CommentLessThanSignBangDash => slow_read_byte!(
            slf,
            match c {
                Some(b'-') => {
                    switch_to!(slf, State::CommentLessThanSignBangDashDash)
                }
                c => {
                    reconsume_in!(slf, c, State::CommentEndDash)
                }
            }
        ),
        State::CommentLessThanSignBangDashDash => slow_read_byte!(
            slf,
            match c {
                c @ (Some(b'>') | None) => {
                    reconsume_in!(slf, c, State::CommentEnd)
                }
                c => {
                    error!(slf, Error::NestedComment);
                    reconsume_in!(slf, c, State::CommentEnd)
                }
            }
        ),
        State::CommentEndDash => slow_read_byte!(
            slf,
            match c {
                Some(b'-') => {
                    switch_to!(slf, State::CommentEnd)
                }
                None => {
                    error!(slf, Error::EofInComment);
                    slf.emitter.emit_current_comment();
                    eof!()
                }
                c => {
                    slf.emitter.push_comment(b"-");
                    reconsume_in!(slf, c, State::Comment)
                }
            }
        ),
        State::CommentEnd => slow_read_byte!(
            slf,
            match c {
                Some(b'>') => {
                    slf.emitter.emit_current_comment();
                    switch_to!(slf, State::Data)
                }
                Some(b'!') => {
                    switch_to!(slf, State::CommentEndBang)
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
                    reconsume_in!(slf, c, State::Comment)
                }
            }
        ),
        State::CommentEndBang => slow_read_byte!(
            slf,
            match c {
                Some(b'-') => {
                    slf.emitter.push_comment(b"--!");
                    switch_to!(slf, State::CommentEndDash)
                }
                Some(b'>') => {
                    error!(slf, Error::IncorrectlyClosedComment);
                    slf.emitter.emit_current_comment();
                    switch_to!(slf, State::Data)
                }
                None => {
                    error!(slf, Error::EofInComment);
                    slf.emitter.emit_current_comment();
                    eof!()
                }
                c @ Some(_) => {
                    slf.emitter.push_comment(b"--!");
                    reconsume_in!(slf, c, State::Comment)
                }
            }
        ),
        State::Doctype => slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => {
                    switch_to!(slf, State::BeforeDoctypeName)
                }
                c @ Some(b'>') => {
                    reconsume_in!(slf, c, State::BeforeDoctypeName)
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
                    reconsume_in!(slf, c, State::BeforeDoctypeName)
                }
            }
        ),
        State::BeforeDoctypeName => slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => cont!(),
                Some(b'\0') => {
                    error!(slf, Error::UnexpectedNullCharacter);
                    slf.emitter.init_doctype();
                    slf.emitter.push_doctype_name("\u{fffd}".as_bytes());
                    switch_to!(slf, State::DoctypeName)
                }
                Some(b'>') => {
                    error!(slf, Error::MissingDoctypeName);
                    slf.emitter.init_doctype();
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    switch_to!(slf, State::Data)
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
                    switch_to!(slf, State::DoctypeName)
                }
            }
        ),
        State::DoctypeName => fast_read_char!(
            slf,
            match xs {
                Some(b"\t" | b"\x0A" | b"\x0C" | b" ") => {
                    switch_to!(slf, State::AfterDoctypeName)
                }
                Some(b">") => {
                    slf.emitter.emit_current_doctype();
                    switch_to!(slf, State::Data)
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
        ),
        State::AfterDoctypeName => slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => cont!(),
                Some(b'>') => {
                    slf.emitter.emit_current_doctype();
                    switch_to!(slf, State::Data)
                }
                None => {
                    error!(slf, Error::EofInDoctype);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    eof!()
                }
                Some(b'p' | b'P')
                    if slf
                        .reader
                        .try_read_string(&mut slf.validator, "ublic", false)? =>
                {
                    switch_to!(slf, State::AfterDoctypePublicKeyword)
                }
                Some(b's' | b'S')
                    if slf
                        .reader
                        .try_read_string(&mut slf.validator, "ystem", false)? =>
                {
                    switch_to!(slf, State::AfterDoctypeSystemKeyword)
                }
                c @ Some(_) => {
                    error!(slf, Error::InvalidCharacterSequenceAfterDoctypeName);
                    slf.emitter.set_force_quirks();
                    reconsume_in!(slf, c, State::BogusDoctype)
                }
            }
        ),
        State::AfterDoctypePublicKeyword => slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => {
                    switch_to!(slf, State::BeforeDoctypePublicIdentifier)
                }
                Some(b'"') => {
                    error!(slf, Error::MissingWhitespaceAfterDoctypePublicKeyword);
                    slf.emitter.set_doctype_public_identifier(b"");
                    switch_to!(slf, State::DoctypePublicIdentifierDoubleQuoted)
                }
                Some(b'\'') => {
                    error!(slf, Error::MissingWhitespaceAfterDoctypePublicKeyword);
                    slf.emitter.set_doctype_public_identifier(b"");
                    switch_to!(slf, State::DoctypePublicIdentifierSingleQuoted)
                }
                Some(b'>') => {
                    error!(slf, Error::MissingDoctypePublicIdentifier);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    switch_to!(slf, State::Data)
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
                    reconsume_in!(slf, c, State::BogusDoctype)
                }
            }
        ),
        State::BeforeDoctypePublicIdentifier => slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => cont!(),
                Some(b'"') => {
                    slf.emitter.set_doctype_public_identifier(b"");
                    switch_to!(slf, State::DoctypePublicIdentifierDoubleQuoted)
                }
                Some(b'\'') => {
                    slf.emitter.set_doctype_public_identifier(b"");
                    switch_to!(slf, State::DoctypePublicIdentifierSingleQuoted)
                }
                Some(b'>') => {
                    error!(slf, Error::MissingDoctypePublicIdentifier);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    switch_to!(slf, State::Data)
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
                    reconsume_in!(slf, c, State::BogusDoctype)
                }
            }
        ),
        State::DoctypePublicIdentifierDoubleQuoted => fast_read_char!(
            slf,
            match xs {
                Some(b"\"") => {
                    switch_to!(slf, State::AfterDoctypePublicIdentifier)
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
                    switch_to!(slf, State::Data)
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
        ),
        State::DoctypePublicIdentifierSingleQuoted => fast_read_char!(
            slf,
            match xs {
                Some(b"'") => {
                    switch_to!(slf, State::AfterDoctypePublicIdentifier)
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
                    switch_to!(slf, State::Data)
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
        ),
        State::AfterDoctypePublicIdentifier => slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => {
                    switch_to!(slf, State::BetweenDoctypePublicAndSystemIdentifiers)
                }
                Some(b'>') => {
                    slf.emitter.emit_current_doctype();
                    switch_to!(slf, State::Data)
                }
                Some(b'"') => {
                    error!(
                        slf,
                        Error::MissingWhitespaceBetweenDoctypePublicAndSystemIdentifiers
                    );
                    slf.emitter.set_doctype_system_identifier(b"");
                    switch_to!(slf, State::DoctypeSystemIdentifierDoubleQuoted)
                }
                Some(b'\'') => {
                    error!(
                        slf,
                        Error::MissingWhitespaceBetweenDoctypePublicAndSystemIdentifiers
                    );
                    slf.emitter.set_doctype_system_identifier(b"");
                    switch_to!(slf, State::DoctypeSystemIdentifierSingleQuoted)
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
                    reconsume_in!(slf, c, State::BogusDoctype)
                }
            }
        ),
        State::BetweenDoctypePublicAndSystemIdentifiers => slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => cont!(),
                Some(b'>') => {
                    slf.emitter.emit_current_doctype();
                    switch_to!(slf, State::Data)
                }
                Some(b'"') => {
                    slf.emitter.set_doctype_system_identifier(b"");
                    switch_to!(slf, State::DoctypeSystemIdentifierDoubleQuoted)
                }
                Some(b'\'') => {
                    slf.emitter.set_doctype_system_identifier(b"");
                    switch_to!(slf, State::DoctypeSystemIdentifierSingleQuoted)
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
                    reconsume_in!(slf, c, State::BogusDoctype)
                }
            }
        ),
        State::AfterDoctypeSystemKeyword => slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => {
                    switch_to!(slf, State::BeforeDoctypeSystemIdentifier)
                }
                Some(b'"') => {
                    error!(slf, Error::MissingWhitespaceAfterDoctypeSystemKeyword);
                    slf.emitter.set_doctype_system_identifier(b"");
                    switch_to!(slf, State::DoctypeSystemIdentifierDoubleQuoted)
                }
                Some(b'\'') => {
                    error!(slf, Error::MissingWhitespaceAfterDoctypeSystemKeyword);
                    slf.emitter.set_doctype_system_identifier(b"");
                    switch_to!(slf, State::DoctypeSystemIdentifierSingleQuoted)
                }
                Some(b'>') => {
                    error!(slf, Error::MissingDoctypeSystemIdentifier);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    switch_to!(slf, State::Data)
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
                    reconsume_in!(slf, c, State::BogusDoctype)
                }
            }
        ),
        State::BeforeDoctypeSystemIdentifier => slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => cont!(),
                Some(b'"') => {
                    slf.emitter.set_doctype_system_identifier(b"");
                    switch_to!(slf, State::DoctypeSystemIdentifierDoubleQuoted)
                }
                Some(b'\'') => {
                    slf.emitter.set_doctype_system_identifier(b"");
                    switch_to!(slf, State::DoctypeSystemIdentifierSingleQuoted)
                }
                Some(b'>') => {
                    error!(slf, Error::MissingDoctypeSystemIdentifier);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    switch_to!(slf, State::Data)
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
                    reconsume_in!(slf, c, State::BogusDoctype)
                }
            }
        ),
        State::DoctypeSystemIdentifierDoubleQuoted => fast_read_char!(
            slf,
            match xs {
                Some(b"\"") => {
                    switch_to!(slf, State::AfterDoctypeSystemIdentifier)
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
                    switch_to!(slf, State::Data)
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
        ),
        State::DoctypeSystemIdentifierSingleQuoted => fast_read_char!(
            slf,
            match xs {
                Some(b"\'") => {
                    switch_to!(slf, State::AfterDoctypeSystemIdentifier)
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
                    switch_to!(slf, State::Data)
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
        ),
        State::AfterDoctypeSystemIdentifier => slow_read_byte!(
            slf,
            match c {
                Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => cont!(),
                Some(b'>') => {
                    slf.emitter.emit_current_doctype();
                    switch_to!(slf, State::Data)
                }
                None => {
                    error!(slf, Error::EofInDoctype);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    eof!()
                }
                c @ Some(_) => {
                    error!(slf, Error::UnexpectedCharacterAfterDoctypeSystemIdentifier);
                    reconsume_in!(slf, c, State::BogusDoctype)
                }
            }
        ),
        State::BogusDoctype => fast_read_char!(
            slf,
            match xs {
                Some(b">") => {
                    slf.emitter.emit_current_doctype();
                    switch_to!(slf, State::Data)
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
        ),
        State::CdataSection => fast_read_char!(
            slf,
            match xs {
                Some(b"]") => {
                    switch_to!(slf, State::CdataSectionBracket)
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
        ),
        State::CdataSectionBracket => slow_read_byte!(
            slf,
            match c {
                Some(b']') => {
                    switch_to!(slf, State::CdataSectionEnd)
                }
                c => {
                    slf.emitter.emit_string(b"]");
                    reconsume_in!(slf, c, State::CdataSection)
                }
            }
        ),
        State::CdataSectionEnd => slow_read_byte!(
            slf,
            match c {
                Some(b']') => {
                    slf.emitter.emit_string(b"]");
                    cont!()
                }
                Some(b'>') => {
                    switch_to!(slf, State::Data)
                }
                c => {
                    slf.emitter.emit_string(b"]]");
                    reconsume_in!(slf, c, State::CdataSection)
                }
            }
        ),
        State::CharacterReference => {
            slf.machine_helper.temporary_buffer.clear();
            slf.machine_helper.temporary_buffer.push(b'&');

            slow_read_byte!(
                slf,
                match c {
                    Some(x) if x.is_ascii_alphanumeric() => {
                        reconsume_in!(slf, Some(x), State::NamedCharacterReference)
                    }
                    Some(b'#') => {
                        slf.machine_helper.temporary_buffer.push(b'#');
                        switch_to!(slf, State::NumericCharacterReference)
                    }
                    c => {
                        slf.machine_helper
                            .flush_code_points_consumed_as_character_reference(&mut slf.emitter);
                        reconsume_in!(slf, c, slf.machine_helper.pop_return_state())
                    }
                }
            )
        }
        State::NamedCharacterReference => {
            let c = read_byte!(slf)?;

            let char_ref = match c {
                Some(x) => try_read_character_reference(x as char, |x| {
                    slf.reader.try_read_string(&mut slf.validator, x, true)
                })?
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

                slf.machine_helper
                    .flush_code_points_consumed_as_character_reference(&mut slf.emitter);
                reconsume_in!(slf, next_character, slf.machine_helper.pop_return_state())
            } else {
                slf.machine_helper
                    .flush_code_points_consumed_as_character_reference(&mut slf.emitter);
                reconsume_in!(slf, c, State::AmbiguousAmpersand)
            }
        }
        State::AmbiguousAmpersand => slow_read_byte!(
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
                    reconsume_in!(slf, c, slf.machine_helper.pop_return_state())
                }
                c => {
                    reconsume_in!(slf, c, slf.machine_helper.pop_return_state())
                }
            }
        ),
        State::NumericCharacterReference => {
            slf.machine_helper.character_reference_code = 0;

            slow_read_byte!(
                slf,
                match c {
                    Some(x @ (b'x' | b'X')) => {
                        slf.machine_helper.temporary_buffer.push(x);
                        switch_to!(slf, State::HexadecimalCharacterReferenceStart)
                    }
                    Some(x @ b'0'..=b'9') => {
                        reconsume_in!(slf, Some(x), State::DecimalCharacterReference)
                    }
                    c => {
                        error!(slf, Error::AbsenceOfDigitsInNumericCharacterReference);
                        slf.machine_helper
                            .flush_code_points_consumed_as_character_reference(&mut slf.emitter);
                        reconsume_in!(slf, c, slf.machine_helper.pop_return_state())
                    }
                }
            )
        }
        State::HexadecimalCharacterReferenceStart => slow_read_byte!(
            slf,
            match c {
                c @ Some(b'0'..=b'9' | b'A'..=b'F' | b'a'..=b'f') => {
                    reconsume_in!(slf, c, State::HexadecimalCharacterReference)
                }
                c => {
                    error!(slf, Error::AbsenceOfDigitsInNumericCharacterReference);
                    slf.machine_helper
                        .flush_code_points_consumed_as_character_reference(&mut slf.emitter);
                    reconsume_in!(slf, c, slf.machine_helper.pop_return_state())
                }
            }
        ),
        State::HexadecimalCharacterReference => slow_read_byte!(
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
                    switch_to!(slf, State::NumericCharacterReferenceEnd)
                }
                c => {
                    error!(slf, Error::MissingSemicolonAfterCharacterReference);
                    reconsume_in!(slf, c, State::NumericCharacterReferenceEnd)
                }
            }
        ),
        State::DecimalCharacterReference => slow_read_byte!(
            slf,
            match c {
                Some(x @ b'0'..=b'9') => {
                    mutate_character_reference!(slf, *10 + x - 0x0030);
                    cont!()
                }
                Some(b';') => {
                    switch_to!(slf, State::NumericCharacterReferenceEnd)
                }
                c => {
                    error!(slf, Error::MissingSemicolonAfterCharacterReference);
                    reconsume_in!(slf, c, State::NumericCharacterReferenceEnd)
                }
            }
        ),
        State::NumericCharacterReferenceEnd => {
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
        }
    }
}
