use crate::entities::try_read_character_reference;
use crate::read_helper::fast_read_char;
use crate::utils::{
    control_pat, ctostr, noncharacter_pat, surrogate_pat, with_lowercase_str, ControlToken, State,
};
use crate::{Emitter, Error, Reader, Tokenizer};

// Note: This is not implemented as a method on Tokenizer because there's fields on Tokenizer that
// should not be available in this method, such as Tokenizer.to_reconsume or the Reader instance
#[inline]
pub fn consume<R: Reader, E: Emitter>(slf: &mut Tokenizer<R, E>) -> Result<ControlToken, R::Error> {
    macro_rules! mutate_character_reference {
        (* $mul:literal + $x:ident - $sub:literal) => {
            match slf
                .machine_helper
                .character_reference_code
                .checked_mul($mul)
                .and_then(|cr| cr.checked_add($x as u32 - $sub))
            {
                Some(cr) => slf.machine_helper.character_reference_code = cr,
                None => {
                    // provoke err
                    slf.machine_helper.character_reference_code = 0x110000;
                }
            };
        };
    }

    macro_rules! switch_to {
        ($state:expr) => {{
            slf.machine_helper.switch_to($state);
            cont!()
        }};
    }

    macro_rules! enter_state {
        ($state:expr) => {{
            slf.machine_helper.enter_state($state);
            cont!()
        }};
    }

    macro_rules! exit_state {
        () => {{
            slf.machine_helper.exit_state();
            cont!()
        }};
    }

    macro_rules! reconsume_in {
        ($c:expr, $state:expr) => {{
            let new_state = $state;
            let c = $c;
            slf.reader.unread_byte(c);
            slf.machine_helper.switch_to(new_state);
            cont!()
        }};
    }

    macro_rules! cont {
        () => {{
            return Ok(ControlToken::Continue);
        }};
    }

    macro_rules! eof {
        () => {{
            return Ok(ControlToken::Eof);
        }};
    }

    macro_rules! read_byte {
        () => {
            slf.reader.read_byte(&mut slf.validator, &mut slf.emitter)
        };
    }

    macro_rules! error {
        ($e:expr) => {
            slf.validator.set_character_error(&mut slf.emitter, $e);
        };
    }

    macro_rules! error_immediate {
        ($e:expr) => {
            error!($e);
            slf.validator.flush_character_error(&mut slf.emitter);
        };
    }

    match slf.machine_helper.state() {
        State::Data => fast_read_char!(
            slf,
            match xs {
                Some(b"&") => {
                    enter_state!(State::CharacterReference)
                }
                Some(b"<") => {
                    switch_to!(State::TagOpen)
                }
                Some(b"\0") => {
                    error_immediate!(Error::UnexpectedNullCharacter);
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
        ),

        State::RcData => fast_read_char!(
            slf,
            match xs {
                Some(b"&") => {
                    enter_state!(State::CharacterReference)
                }
                Some(b"<") => {
                    switch_to!(State::RcDataLessThanSign)
                }
                Some(b"\0") => {
                    error!(Error::UnexpectedNullCharacter);
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
        State::RawText => fast_read_char!(
            slf,
            match xs {
                Some(b"<") => {
                    switch_to!(State::RawTextLessThanSign)
                }
                Some(b"\0") => {
                    error!(Error::UnexpectedNullCharacter);
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
        State::ScriptData => fast_read_char!(
            slf,
            match xs {
                Some(b"<") => {
                    switch_to!(State::ScriptDataLessThanSign)
                }
                Some(b"\0") => {
                    error!(Error::UnexpectedNullCharacter);
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
                    error!(Error::UnexpectedNullCharacter);
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
        State::TagOpen => match read_byte!()? {
            Some(b'!') => {
                switch_to!(State::MarkupDeclarationOpen)
            }
            Some(b'/') => {
                switch_to!(State::EndTagOpen)
            }
            Some(x) if x.is_ascii_alphabetic() => {
                slf.emitter.init_start_tag();
                reconsume_in!(Some(x), State::TagName)
            }
            c @ Some(b'?') => {
                error!(Error::UnexpectedQuestionMarkInsteadOfTagName);
                slf.emitter.init_comment();
                reconsume_in!(c, State::BogusComment)
            }
            None => {
                error!(Error::EofBeforeTagName);
                slf.emitter.emit_string(b"<");
                eof!()
            }
            c @ Some(_) => {
                error!(Error::InvalidFirstCharacterOfTagName);
                slf.emitter.emit_string(b"<");
                reconsume_in!(c, State::Data)
            }
        },
        State::EndTagOpen => match read_byte!()? {
            Some(x) if x.is_ascii_alphabetic() => {
                slf.emitter.init_end_tag();
                reconsume_in!(Some(x), State::TagName)
            }
            Some(b'>') => {
                error!(Error::MissingEndTagName);
                switch_to!(State::Data)
            }
            None => {
                error!(Error::EofBeforeTagName);
                slf.emitter.emit_string(b"</");
                eof!()
            }
            Some(x) => {
                error!(Error::InvalidFirstCharacterOfTagName);
                slf.emitter.init_comment();
                reconsume_in!(Some(x), State::BogusComment)
            }
        },
        State::TagName => fast_read_char!(
            slf,
            match xs {
                Some(b"\t" | b"\x0A" | b"\x0C" | b" ") => {
                    switch_to!(State::BeforeAttributeName)
                }
                Some(b"/") => {
                    switch_to!(State::SelfClosingStartTag)
                }
                Some(b">") => {
                    slf.emitter.emit_current_tag();
                    switch_to!(State::Data)
                }
                Some(b"\0") => {
                    error!(Error::UnexpectedNullCharacter);
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
                    error!(Error::EofInTag);
                    eof!()
                }
            }
        ),
        State::RcDataLessThanSign => match read_byte!()? {
            Some(b'/') => {
                slf.machine_helper.temporary_buffer.clear();
                switch_to!(State::RcDataEndTagOpen)
            }
            c => {
                slf.emitter.emit_string(b"<");
                reconsume_in!(c, State::RcData)
            }
        },
        State::RcDataEndTagOpen => match read_byte!()? {
            Some(x) if x.is_ascii_alphabetic() => {
                slf.emitter.init_end_tag();
                reconsume_in!(Some(x), State::RcDataEndTagName)
            }
            c => {
                slf.emitter.emit_string(b"</");
                reconsume_in!(c, State::RcData)
            }
        },
        State::RcDataEndTagName => match read_byte!()? {
            Some(b'\t' | b'\x0A' | b'\x0C' | b' ')
                if slf.emitter.current_is_appropriate_end_tag_token() =>
            {
                switch_to!(State::BeforeAttributeName)
            }
            Some(b'/') if slf.emitter.current_is_appropriate_end_tag_token() => {
                switch_to!(State::SelfClosingStartTag)
            }
            Some(b'>') if slf.emitter.current_is_appropriate_end_tag_token() => {
                slf.emitter.emit_current_tag();
                switch_to!(State::Data)
            }
            Some(x) if x.is_ascii_alphabetic() => {
                slf.emitter.push_tag_name(&[x.to_ascii_lowercase()]);
                slf.machine_helper.temporary_buffer.push(x as u8);
                cont!()
            }
            c => {
                slf.emitter.emit_string(b"</");
                slf.machine_helper.flush_buffer_characters(&mut slf.emitter);
                reconsume_in!(c, State::RcData)
            }
        },
        State::RawTextLessThanSign => match read_byte!()? {
            Some(b'/') => {
                slf.machine_helper.temporary_buffer.clear();
                switch_to!(State::RawTextEndTagOpen)
            }
            c => {
                slf.emitter.emit_string(b"<");
                reconsume_in!(c, State::RawText)
            }
        },
        State::RawTextEndTagOpen => match read_byte!()? {
            Some(x) if x.is_ascii_alphabetic() => {
                slf.emitter.init_end_tag();
                reconsume_in!(Some(x), State::RawTextEndTagName)
            }
            c => {
                slf.emitter.emit_string(b"</");
                reconsume_in!(c, State::RawText)
            }
        },
        State::RawTextEndTagName => match read_byte!()? {
            Some(b'\t' | b'\x0A' | b'\x0C' | b' ')
                if slf.emitter.current_is_appropriate_end_tag_token() =>
            {
                switch_to!(State::BeforeAttributeName)
            }
            Some(b'/') if slf.emitter.current_is_appropriate_end_tag_token() => {
                switch_to!(State::SelfClosingStartTag)
            }
            Some(b'>') if slf.emitter.current_is_appropriate_end_tag_token() => {
                slf.emitter.emit_current_tag();
                switch_to!(State::Data)
            }
            Some(x) if x.is_ascii_alphabetic() => {
                slf.emitter.push_tag_name(&[x.to_ascii_lowercase()]);
                slf.machine_helper.temporary_buffer.push(x as u8);
                cont!()
            }
            c => {
                slf.emitter.emit_string(b"</");
                slf.machine_helper.flush_buffer_characters(&mut slf.emitter);
                reconsume_in!(c, State::RawText)
            }
        },
        State::ScriptDataLessThanSign => match read_byte!()? {
            Some(b'/') => {
                slf.machine_helper.temporary_buffer.clear();
                switch_to!(State::ScriptDataEndTagOpen)
            }
            Some(b'!') => {
                slf.emitter.emit_string(b"<!");
                switch_to!(State::ScriptDataEscapeStart)
            }
            c => {
                slf.emitter.emit_string(b"<");
                reconsume_in!(c, State::ScriptData)
            }
        },
        State::ScriptDataEndTagOpen => match read_byte!()? {
            Some(x) if x.is_ascii_alphabetic() => {
                slf.emitter.init_end_tag();
                reconsume_in!(Some(x), State::ScriptDataEndTagName)
            }
            c => {
                slf.emitter.emit_string(b"</");
                reconsume_in!(c, State::ScriptData)
            }
        },
        State::ScriptDataEndTagName => match read_byte!()? {
            Some(b'\t' | b'\x0A' | b'\x0C' | b' ')
                if slf.emitter.current_is_appropriate_end_tag_token() =>
            {
                switch_to!(State::BeforeAttributeName)
            }
            Some(b'/') if slf.emitter.current_is_appropriate_end_tag_token() => {
                switch_to!(State::SelfClosingStartTag)
            }
            Some(b'>') if slf.emitter.current_is_appropriate_end_tag_token() => {
                slf.emitter.emit_current_tag();
                switch_to!(State::Data)
            }
            Some(x) if x.is_ascii_alphabetic() => {
                slf.emitter.push_tag_name(&[x.to_ascii_lowercase()]);
                slf.machine_helper
                    .temporary_buffer
                    .push(x.to_ascii_lowercase() as u8);
                cont!()
            }
            c => {
                slf.emitter.emit_string(b"</");
                slf.machine_helper.flush_buffer_characters(&mut slf.emitter);
                reconsume_in!(c, State::Data)
            }
        },
        State::ScriptDataEscapeStart => match read_byte!()? {
            Some(b'-') => {
                slf.emitter.emit_string(b"-");
                switch_to!(State::ScriptDataEscapeStartDash)
            }
            c => {
                reconsume_in!(c, State::ScriptData)
            }
        },
        State::ScriptDataEscapeStartDash => match read_byte!()? {
            Some(b'-') => {
                slf.emitter.emit_string(b"-");
                switch_to!(State::ScriptDataEscapedDashDash)
            }
            c => {
                reconsume_in!(c, State::ScriptData)
            }
        },
        State::ScriptDataEscaped => fast_read_char!(
            slf,
            match xs {
                Some(b"-") => {
                    slf.emitter.emit_string(b"-");
                    switch_to!(State::ScriptDataEscapedDash)
                }
                Some(b"<") => {
                    switch_to!(State::ScriptDataEscapedLessThanSign)
                }
                Some(b"\0") => {
                    error!(Error::UnexpectedNullCharacter);
                    slf.emitter.emit_string("\u{fffd}".as_bytes());
                    cont!()
                }
                Some(xs) => {
                    slf.emitter.emit_string(xs);
                    cont!()
                }
                None => {
                    error!(Error::EofInScriptHtmlCommentLikeText);
                    eof!()
                }
            }
        ),
        State::ScriptDataEscapedDash => match read_byte!()? {
            Some(b'-') => {
                slf.emitter.emit_string(b"-");
                switch_to!(State::ScriptDataEscapedDashDash)
            }
            Some(b'<') => {
                switch_to!(State::ScriptDataEscapedLessThanSign)
            }
            Some(b'\0') => {
                error!(Error::UnexpectedNullCharacter);
                slf.emitter.emit_string("\u{fffd}".as_bytes());
                switch_to!(State::ScriptDataEscaped)
            }
            Some(x) => {
                slf.emitter.emit_string(&[x]);
                switch_to!(State::ScriptDataEscaped)
            }
            None => {
                error!(Error::EofInScriptHtmlCommentLikeText);
                eof!()
            }
        },
        State::ScriptDataEscapedDashDash => match read_byte!()? {
            Some(b'-') => {
                slf.emitter.emit_string(b"-");
                cont!()
            }
            Some(b'<') => {
                switch_to!(State::ScriptDataEscapedLessThanSign)
            }
            Some(b'>') => {
                slf.emitter.emit_string(b">");
                switch_to!(State::ScriptData)
            }
            Some(b'\0') => {
                error!(Error::UnexpectedNullCharacter);
                slf.emitter.emit_string("\u{fffd}".as_bytes());
                switch_to!(State::ScriptDataEscaped)
            }
            Some(x) => {
                slf.emitter.emit_string(&[x]);
                switch_to!(State::ScriptDataEscaped)
            }
            None => {
                error!(Error::EofInScriptHtmlCommentLikeText);
                eof!()
            }
        },
        State::ScriptDataEscapedLessThanSign => match read_byte!()? {
            Some(b'/') => {
                slf.machine_helper.temporary_buffer.clear();
                switch_to!(State::ScriptDataEscapedEndTagOpen)
            }
            Some(x) if x.is_ascii_alphabetic() => {
                slf.machine_helper.temporary_buffer.clear();
                slf.emitter.emit_string(b"<");
                reconsume_in!(Some(x), State::ScriptDataDoubleEscapeStart)
            }
            c => {
                slf.emitter.emit_string(b"<");
                reconsume_in!(c, State::ScriptDataEscaped)
            }
        },
        State::ScriptDataEscapedEndTagOpen => match read_byte!()? {
            Some(x) if x.is_ascii_alphabetic() => {
                slf.emitter.init_end_tag();
                reconsume_in!(Some(x), State::ScriptDataEscapedEndTagName)
            }
            c => {
                slf.emitter.emit_string(b"</");
                reconsume_in!(c, State::ScriptDataEscaped)
            }
        },
        State::ScriptDataEscapedEndTagName => match read_byte!()? {
            Some(b'\t' | b'\x0A' | b'\x0C' | b' ')
                if slf.emitter.current_is_appropriate_end_tag_token() =>
            {
                switch_to!(State::BeforeAttributeName)
            }
            Some(b'/') if slf.emitter.current_is_appropriate_end_tag_token() => {
                switch_to!(State::SelfClosingStartTag)
            }
            Some(b'>') if slf.emitter.current_is_appropriate_end_tag_token() => {
                slf.emitter.emit_current_tag();
                switch_to!(State::Data)
            }
            Some(x) if x.is_ascii_alphabetic() => {
                slf.emitter.push_tag_name(&[x.to_ascii_lowercase()]);
                slf.machine_helper.temporary_buffer.extend(&[x]);
                cont!()
            }
            c => {
                slf.emitter.emit_string(b"</");
                slf.machine_helper.flush_buffer_characters(&mut slf.emitter);
                reconsume_in!(c, State::ScriptDataEscaped)
            }
        },
        State::ScriptDataDoubleEscapeStart => match read_byte!()? {
            Some(x @ (b'\t' | b'\x0A' | b'\x0C' | b' ' | b'/' | b'>')) => {
                slf.emitter.emit_string(&[x]);
                if slf.machine_helper.temporary_buffer == b"script" {
                    switch_to!(State::ScriptDataDoubleEscaped)
                } else {
                    switch_to!(State::ScriptDataEscaped)
                }
            }
            Some(x) if x.is_ascii_alphabetic() => {
                slf.machine_helper
                    .temporary_buffer
                    .push(x.to_ascii_lowercase() as u8);
                slf.emitter.emit_string(&[x]);
                cont!()
            }
            c => {
                reconsume_in!(c, State::ScriptDataEscaped)
            }
        },
        State::ScriptDataDoubleEscaped => fast_read_char!(
            slf,
            match xs {
                Some(b"-") => {
                    slf.emitter.emit_string(b"-");
                    switch_to!(State::ScriptDataDoubleEscapedDash)
                }
                Some(b"<") => {
                    slf.emitter.emit_string(b"<");
                    switch_to!(State::ScriptDataDoubleEscapedLessThanSign)
                }
                Some(b"\0") => {
                    error!(Error::UnexpectedNullCharacter);
                    slf.emitter.emit_string("\u{fffd}".as_bytes());
                    cont!()
                }
                Some(xs) => {
                    slf.emitter.emit_string(xs);
                    cont!()
                }
                None => {
                    error!(Error::EofInScriptHtmlCommentLikeText);
                    eof!()
                }
            }
        ),
        State::ScriptDataDoubleEscapedDash => match read_byte!()? {
            Some(b'-') => {
                slf.emitter.emit_string(b"-");
                switch_to!(State::ScriptDataDoubleEscapedDashDash)
            }
            Some(b'<') => {
                slf.emitter.emit_string(b"<");
                switch_to!(State::ScriptDataDoubleEscapedLessThanSign)
            }
            Some(b'\0') => {
                error!(Error::UnexpectedNullCharacter);
                slf.emitter.emit_string("\u{fffd}".as_bytes());
                switch_to!(State::ScriptDataDoubleEscaped)
            }
            Some(x) => {
                slf.emitter.emit_string(&[x]);
                switch_to!(State::ScriptDataDoubleEscaped)
            }
            None => {
                error!(Error::EofInScriptHtmlCommentLikeText);
                eof!()
            }
        },
        State::ScriptDataDoubleEscapedDashDash => match read_byte!()? {
            Some(b'-') => {
                slf.emitter.emit_string(b"-");
                cont!()
            }
            Some(b'<') => {
                slf.emitter.emit_string(b"<");
                switch_to!(State::ScriptDataDoubleEscapedLessThanSign)
            }
            Some(b'>') => {
                slf.emitter.emit_string(b">");
                switch_to!(State::ScriptData)
            }
            Some(b'\0') => {
                error!(Error::UnexpectedNullCharacter);
                slf.emitter.emit_string("\u{fffd}".as_bytes());
                switch_to!(State::ScriptDataDoubleEscaped)
            }
            Some(x) => {
                slf.emitter.emit_string(&[x]);
                switch_to!(State::ScriptDataDoubleEscaped)
            }
            None => {
                error!(Error::EofInScriptHtmlCommentLikeText);
                eof!()
            }
        },
        State::ScriptDataDoubleEscapedLessThanSign => match read_byte!()? {
            Some(b'/') => {
                slf.machine_helper.temporary_buffer.clear();
                slf.emitter.emit_string(b"/");
                switch_to!(State::ScriptDataDoubleEscapeEnd)
            }
            c => {
                reconsume_in!(c, State::ScriptDataDoubleEscaped)
            }
        },
        State::ScriptDataDoubleEscapeEnd => match read_byte!()? {
            Some(x @ (b'\t' | b'\x0A' | b'\x0C' | b' ' | b'/' | b'>')) => {
                slf.emitter.emit_string(&[x]);

                if slf.machine_helper.temporary_buffer == b"script" {
                    switch_to!(State::ScriptDataEscaped)
                } else {
                    switch_to!(State::ScriptDataDoubleEscaped)
                }
            }
            Some(x) if x.is_ascii_alphabetic() => {
                slf.machine_helper
                    .temporary_buffer
                    .push(x.to_ascii_lowercase() as u8);
                slf.emitter.emit_string(&[x]);
                cont!()
            }
            c => {
                reconsume_in!(c, State::ScriptDataDoubleEscaped)
            }
        },
        State::BeforeAttributeName => match read_byte!()? {
            Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => cont!(),
            c @ Some(b'/' | b'>') | c @ None => {
                reconsume_in!(c, State::AfterAttributeName)
            }
            Some(b'=') => {
                error_immediate!(Error::UnexpectedEqualsSignBeforeAttributeName);
                slf.emitter.init_attribute();
                slf.emitter.push_attribute_name("=".as_bytes());
                switch_to!(State::AttributeName)
            }
            Some(x) => {
                slf.emitter.init_attribute();
                reconsume_in!(Some(x), State::AttributeName)
            }
        },
        State::AttributeName => fast_read_char!(
            slf,
            match xs {
                Some(b"\t" | b"\x0A" | b"\x0C" | b" " | b"/" | b">") => {
                    reconsume_in!(Some(xs.unwrap()[0]), State::AfterAttributeName)
                }
                Some(b"=") => {
                    switch_to!(State::BeforeAttributeValue)
                }
                Some(b"\0") => {
                    error!(Error::UnexpectedNullCharacter);
                    slf.emitter.push_attribute_name("\u{fffd}".as_bytes());
                    cont!()
                }
                Some(b"\"" | b"'" | b"<") => {
                    error!(Error::UnexpectedCharacterInAttributeName);
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
                    reconsume_in!(None, State::AfterAttributeName)
                }
            }
        ),
        State::AfterAttributeName => match read_byte!()? {
            Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => cont!(),
            Some(b'/') => {
                switch_to!(State::SelfClosingStartTag)
            }
            Some(b'=') => {
                switch_to!(State::BeforeAttributeValue)
            }
            Some(b'>') => {
                slf.emitter.emit_current_tag();
                switch_to!(State::Data)
            }
            None => {
                error!(Error::EofInTag);
                eof!()
            }
            Some(x) => {
                slf.emitter.init_attribute();
                reconsume_in!(Some(x), State::AttributeName)
            }
        },
        State::BeforeAttributeValue => match read_byte!()? {
            Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => cont!(),
            Some(b'"') => {
                switch_to!(State::AttributeValueDoubleQuoted)
            }
            Some(b'\'') => {
                switch_to!(State::AttributeValueSingleQuoted)
            }
            Some(b'>') => {
                error_immediate!(Error::MissingAttributeValue);
                slf.emitter.emit_current_tag();
                switch_to!(State::Data)
            }
            c => {
                reconsume_in!(c, State::AttributeValueUnquoted)
            }
        },
        State::AttributeValueDoubleQuoted => fast_read_char!(
            slf,
            match xs {
                Some(b"\"") => {
                    switch_to!(State::AfterAttributeValueQuoted)
                }
                Some(b"&") => {
                    enter_state!(State::CharacterReference)
                }
                Some(b"\0") => {
                    error!(Error::UnexpectedNullCharacter);
                    slf.emitter.push_attribute_value("\u{fffd}".as_bytes());
                    cont!()
                }
                Some(xs) => {
                    slf.emitter.push_attribute_value(xs);
                    cont!()
                }
                None => {
                    error!(Error::EofInTag);
                    eof!()
                }
            }
        ),
        State::AttributeValueSingleQuoted => fast_read_char!(
            slf,
            match xs {
                Some(b"'") => {
                    switch_to!(State::AfterAttributeValueQuoted)
                }
                Some(b"&") => {
                    enter_state!(State::CharacterReference)
                }
                Some(b"\0") => {
                    error!(Error::UnexpectedNullCharacter);
                    slf.emitter.push_attribute_value("\u{fffd}".as_bytes());
                    cont!()
                }
                Some(xs) => {
                    slf.emitter.push_attribute_value(xs);
                    cont!()
                }
                None => {
                    error!(Error::EofInTag);
                    eof!()
                }
            }
        ),
        State::AttributeValueUnquoted => fast_read_char!(
            slf,
            match xs {
                Some(b"\t" | b"\x0A" | b"\x0C" | b" ") => {
                    switch_to!(State::BeforeAttributeName)
                }
                Some(b"&") => {
                    enter_state!(State::CharacterReference)
                }
                Some(b">") => {
                    slf.emitter.emit_current_tag();
                    switch_to!(State::Data)
                }
                Some(b"\0") => {
                    error!(Error::UnexpectedNullCharacter);
                    slf.emitter.push_attribute_value("\u{fffd}".as_bytes());
                    cont!()
                }
                Some(b"\"" | b"'" | b"<" | b"=" | b"\x60") => {
                    error!(Error::UnexpectedCharacterInUnquotedAttributeValue);
                    slf.emitter.push_attribute_value(xs.unwrap());
                    cont!()
                }
                Some(xs) => {
                    slf.emitter.push_attribute_value(xs);
                    cont!()
                }
                None => {
                    error!(Error::EofInTag);
                    eof!()
                }
            }
        ),
        State::AfterAttributeValueQuoted => match read_byte!()? {
            c @ (Some(b'\t' | b'\x0A' | b'\x0C' | b' ' | b'/' | b'>') | None) => {
                reconsume_in!(c, State::BeforeAttributeName)
            }
            c => {
                error_immediate!(Error::MissingWhitespaceBetweenAttributes);
                reconsume_in!(c, State::BeforeAttributeName)
            }
        },
        State::SelfClosingStartTag => match read_byte!()? {
            Some(b'>') => {
                slf.emitter.set_self_closing();
                slf.emitter.emit_current_tag();
                switch_to!(State::Data)
            }
            None => {
                error!(Error::EofInTag);
                eof!()
            }
            Some(x) => {
                error_immediate!(Error::UnexpectedSolidusInTag);
                reconsume_in!(Some(x), State::BeforeAttributeName)
            }
        },
        State::BogusComment => fast_read_char!(
            slf,
            match xs {
                Some(b">") => {
                    slf.emitter.emit_current_comment();
                    switch_to!(State::Data)
                }
                Some(b"\0") => {
                    error!(Error::UnexpectedNullCharacter);
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
        State::MarkupDeclarationOpen => {
            {
                match read_byte!()? {
                    Some(b'-') if slf.reader.try_read_string(&mut slf.validator, "-", true)? => {
                        slf.emitter.init_comment();
                        switch_to!(State::CommentStart)
                    }
                    Some(b'd' | b'D')
                        if slf
                            .reader
                            .try_read_string(&mut slf.validator, "octype", false)? =>
                    {
                        switch_to!(State::Doctype)
                    }
                    Some(b'[')
                        if slf
                            .reader
                            .try_read_string(&mut slf.validator, "CDATA[", true)? =>
                    {
                        // missing: check for adjusted current element: we don't have an element stack
                        // at all
                        //
                        // missing: cdata transition
                        //
                        // let's hope that bogus comment can just sort of skip over cdata
                        error!(Error::CdataInHtmlContent);

                        slf.emitter.init_comment();
                        slf.emitter.push_comment(b"[CDATA[");
                        switch_to!(State::BogusComment)
                    }
                    c => {
                        error!(Error::IncorrectlyOpenedComment);
                        slf.emitter.init_comment();
                        reconsume_in!(c, State::BogusComment)
                    }
                }
            }
        }
        State::CommentStart => match read_byte!()? {
            Some(b'-') => {
                switch_to!(State::CommentStartDash)
            }
            Some(b'>') => {
                error!(Error::AbruptClosingOfEmptyComment);
                slf.emitter.emit_current_comment();
                switch_to!(State::Data)
            }
            c => {
                reconsume_in!(c, State::Comment)
            }
        },
        State::CommentStartDash => match read_byte!()? {
            Some(b'-') => {
                switch_to!(State::CommentEnd)
            }
            Some(b'>') => {
                error!(Error::AbruptClosingOfEmptyComment);
                slf.emitter.emit_current_comment();
                switch_to!(State::Data)
            }
            None => {
                error!(Error::EofInComment);
                slf.emitter.emit_current_comment();
                eof!()
            }
            c @ Some(_) => {
                slf.emitter.push_comment(b"-");
                reconsume_in!(c, State::Comment)
            }
        },
        State::Comment => fast_read_char!(
            slf,
            match xs {
                Some(b"<") => {
                    slf.emitter.push_comment(b"<");
                    switch_to!(State::CommentLessThanSign)
                }
                Some(b"-") => {
                    switch_to!(State::CommentEndDash)
                }
                Some(b"\0") => {
                    error!(Error::UnexpectedNullCharacter);
                    slf.emitter.push_comment("\u{fffd}".as_bytes());
                    cont!()
                }
                Some(xs) => {
                    slf.emitter.push_comment(xs);
                    cont!()
                }
                None => {
                    error!(Error::EofInComment);
                    slf.emitter.emit_current_comment();
                    eof!()
                }
            }
        ),
        State::CommentLessThanSign => match read_byte!()? {
            Some(b'!') => {
                slf.emitter.push_comment(b"!");
                switch_to!(State::CommentLessThanSignBang)
            }
            Some(b'<') => {
                slf.emitter.push_comment(b"<");
                cont!()
            }
            c => {
                reconsume_in!(c, State::Comment)
            }
        },
        State::CommentLessThanSignBang => match read_byte!()? {
            Some(b'-') => {
                switch_to!(State::CommentLessThanSignBangDash)
            }
            c => {
                reconsume_in!(c, State::Comment)
            }
        },
        State::CommentLessThanSignBangDash => match read_byte!()? {
            Some(b'-') => {
                switch_to!(State::CommentLessThanSignBangDashDash)
            }
            c => {
                reconsume_in!(c, State::CommentEndDash)
            }
        },
        State::CommentLessThanSignBangDashDash => match read_byte!()? {
            c @ Some(b'>') | c @ None => {
                reconsume_in!(c, State::CommentEnd)
            }
            c => {
                error!(Error::NestedComment);
                reconsume_in!(c, State::CommentEnd)
            }
        },
        State::CommentEndDash => match read_byte!()? {
            Some(b'-') => {
                switch_to!(State::CommentEnd)
            }
            None => {
                error!(Error::EofInComment);
                slf.emitter.emit_current_comment();
                eof!()
            }
            c => {
                slf.emitter.push_comment(b"-");
                reconsume_in!(c, State::Comment)
            }
        },
        State::CommentEnd => match read_byte!()? {
            Some(b'>') => {
                slf.emitter.emit_current_comment();
                switch_to!(State::Data)
            }
            Some(b'!') => {
                switch_to!(State::CommentEndBang)
            }
            Some(b'-') => {
                slf.emitter.push_comment(b"-");
                cont!()
            }
            None => {
                error!(Error::EofInComment);
                slf.emitter.emit_current_comment();
                eof!()
            }
            c @ Some(_) => {
                slf.emitter.push_comment(b"--");
                reconsume_in!(c, State::Comment)
            }
        },
        State::CommentEndBang => match read_byte!()? {
            Some(b'-') => {
                slf.emitter.push_comment(b"--!");
                switch_to!(State::CommentEndDash)
            }
            Some(b'>') => {
                error!(Error::IncorrectlyClosedComment);
                slf.emitter.emit_current_comment();
                switch_to!(State::Data)
            }
            None => {
                error!(Error::EofInComment);
                slf.emitter.emit_current_comment();
                eof!()
            }
            c @ Some(_) => {
                slf.emitter.push_comment(b"--!");
                reconsume_in!(c, State::Comment)
            }
        },
        State::Doctype => match read_byte!()? {
            Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => {
                switch_to!(State::BeforeDoctypeName)
            }
            c @ Some(b'>') => {
                reconsume_in!(c, State::BeforeDoctypeName)
            }
            None => {
                error_immediate!(Error::EofInDoctype);
                slf.emitter.init_doctype();
                slf.emitter.set_force_quirks();
                slf.emitter.emit_current_doctype();
                eof!()
            }
            c @ Some(_) => {
                error!(Error::MissingWhitespaceBeforeDoctypeName);
                reconsume_in!(c, State::BeforeDoctypeName)
            }
        },
        State::BeforeDoctypeName => match read_byte!()? {
            Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => cont!(),
            Some(b'\0') => {
                error!(Error::UnexpectedNullCharacter);
                slf.emitter.init_doctype();
                slf.emitter.push_doctype_name("\u{fffd}".as_bytes());
                switch_to!(State::DoctypeName)
            }
            Some(b'>') => {
                error!(Error::MissingDoctypeName);
                slf.emitter.init_doctype();
                slf.emitter.set_force_quirks();
                slf.emitter.emit_current_doctype();
                switch_to!(State::Data)
            }
            None => {
                error!(Error::EofInDoctype);
                slf.emitter.init_doctype();
                slf.emitter.set_force_quirks();
                slf.emitter.emit_current_doctype();
                eof!()
            }
            Some(x) => {
                slf.emitter.init_doctype();
                slf.emitter.push_doctype_name(&[x.to_ascii_lowercase()]);
                switch_to!(State::DoctypeName)
            }
        },
        State::DoctypeName => fast_read_char!(
            slf,
            match xs {
                Some(b"\t" | b"\x0A" | b"\x0C" | b" ") => {
                    switch_to!(State::AfterDoctypeName)
                }
                Some(b">") => {
                    slf.emitter.emit_current_doctype();
                    switch_to!(State::Data)
                }
                Some(b"\0") => {
                    error!(Error::UnexpectedNullCharacter);
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
                    error!(Error::EofInDoctype);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    eof!()
                }
            }
        ),
        State::AfterDoctypeName => match read_byte!()? {
            Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => cont!(),
            Some(b'>') => {
                slf.emitter.emit_current_doctype();
                switch_to!(State::Data)
            }
            None => {
                error!(Error::EofInDoctype);
                slf.emitter.set_force_quirks();
                slf.emitter.emit_current_doctype();
                eof!()
            }
            Some(b'p' | b'P')
                if slf
                    .reader
                    .try_read_string(&mut slf.validator, "ublic", false)? =>
            {
                switch_to!(State::AfterDoctypePublicKeyword)
            }
            Some(b's' | b'S')
                if slf
                    .reader
                    .try_read_string(&mut slf.validator, "ystem", false)? =>
            {
                switch_to!(State::AfterDoctypeSystemKeyword)
            }
            c @ Some(_) => {
                error!(Error::InvalidCharacterSequenceAfterDoctypeName);
                slf.emitter.set_force_quirks();
                reconsume_in!(c, State::BogusDoctype)
            }
        },
        State::AfterDoctypePublicKeyword => match read_byte!()? {
            Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => {
                switch_to!(State::BeforeDoctypePublicIdentifier)
            }
            Some(b'"') => {
                error!(Error::MissingWhitespaceAfterDoctypePublicKeyword);
                slf.emitter.set_doctype_public_identifier(b"");
                switch_to!(State::DoctypePublicIdentifierDoubleQuoted)
            }
            Some(b'\'') => {
                error!(Error::MissingWhitespaceAfterDoctypePublicKeyword);
                slf.emitter.set_doctype_public_identifier(b"");
                switch_to!(State::DoctypePublicIdentifierSingleQuoted)
            }
            Some(b'>') => {
                error!(Error::MissingDoctypePublicIdentifier);
                slf.emitter.set_force_quirks();
                slf.emitter.emit_current_doctype();
                switch_to!(State::Data)
            }
            None => {
                error!(Error::EofInDoctype);
                slf.emitter.set_force_quirks();
                slf.emitter.emit_current_doctype();
                eof!()
            }
            c @ Some(_) => {
                error!(Error::MissingQuoteBeforeDoctypePublicIdentifier);
                slf.emitter.set_force_quirks();
                reconsume_in!(c, State::BogusDoctype)
            }
        },
        State::BeforeDoctypePublicIdentifier => match read_byte!()? {
            Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => cont!(),
            Some(b'"') => {
                slf.emitter.set_doctype_public_identifier(b"");
                switch_to!(State::DoctypePublicIdentifierDoubleQuoted)
            }
            Some(b'\'') => {
                slf.emitter.set_doctype_public_identifier(b"");
                switch_to!(State::DoctypePublicIdentifierSingleQuoted)
            }
            Some(b'>') => {
                error!(Error::MissingDoctypePublicIdentifier);
                slf.emitter.set_force_quirks();
                slf.emitter.emit_current_doctype();
                switch_to!(State::Data)
            }
            None => {
                error!(Error::EofInDoctype);
                slf.emitter.set_force_quirks();
                slf.emitter.emit_current_doctype();
                eof!()
            }
            c @ Some(_) => {
                error!(Error::MissingQuoteBeforeDoctypePublicIdentifier);
                slf.emitter.set_force_quirks();
                reconsume_in!(c, State::BogusDoctype)
            }
        },
        State::DoctypePublicIdentifierDoubleQuoted => fast_read_char!(
            slf,
            match xs {
                Some(b"\"") => {
                    switch_to!(State::AfterDoctypePublicIdentifier)
                }
                Some(b"\0") => {
                    error!(Error::UnexpectedNullCharacter);
                    slf.emitter
                        .push_doctype_public_identifier("\u{fffd}".as_bytes());
                    cont!()
                }
                Some(b">") => {
                    error!(Error::AbruptDoctypePublicIdentifier);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    switch_to!(State::Data)
                }
                Some(xs) => {
                    slf.emitter.push_doctype_public_identifier(xs);
                    cont!()
                }
                None => {
                    error!(Error::EofInDoctype);
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
                    switch_to!(State::AfterDoctypePublicIdentifier)
                }
                Some(b"\0") => {
                    error!(Error::UnexpectedNullCharacter);
                    slf.emitter
                        .push_doctype_public_identifier("\u{fffd}".as_bytes());
                    cont!()
                }
                Some(b">") => {
                    error!(Error::AbruptDoctypePublicIdentifier);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    switch_to!(State::Data)
                }
                Some(xs) => {
                    slf.emitter.push_doctype_public_identifier(xs);
                    cont!()
                }
                None => {
                    error!(Error::EofInDoctype);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    eof!()
                }
            }
        ),
        State::AfterDoctypePublicIdentifier => match read_byte!()? {
            Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => {
                switch_to!(State::BetweenDoctypePublicAndSystemIdentifiers)
            }
            Some(b'>') => {
                slf.emitter.emit_current_doctype();
                switch_to!(State::Data)
            }
            Some(b'"') => {
                error!(Error::MissingWhitespaceBetweenDoctypePublicAndSystemIdentifiers);
                slf.emitter.set_doctype_system_identifier(b"");
                switch_to!(State::DoctypeSystemIdentifierDoubleQuoted)
            }
            Some(b'\'') => {
                error!(Error::MissingWhitespaceBetweenDoctypePublicAndSystemIdentifiers);
                slf.emitter.set_doctype_system_identifier(b"");
                switch_to!(State::DoctypeSystemIdentifierSingleQuoted)
            }
            None => {
                error!(Error::EofInDoctype);
                slf.emitter.set_force_quirks();
                slf.emitter.emit_current_doctype();
                eof!()
            }
            c @ Some(_) => {
                error!(Error::MissingQuoteBeforeDoctypeSystemIdentifier);
                slf.emitter.set_force_quirks();
                reconsume_in!(c, State::BogusDoctype)
            }
        },
        State::BetweenDoctypePublicAndSystemIdentifiers => match read_byte!()? {
            Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => cont!(),
            Some(b'>') => {
                slf.emitter.emit_current_doctype();
                switch_to!(State::Data)
            }
            Some(b'"') => {
                slf.emitter.set_doctype_system_identifier(b"");
                switch_to!(State::DoctypeSystemIdentifierDoubleQuoted)
            }
            Some(b'\'') => {
                slf.emitter.set_doctype_system_identifier(b"");
                switch_to!(State::DoctypeSystemIdentifierSingleQuoted)
            }
            None => {
                error!(Error::EofInDoctype);
                slf.emitter.set_force_quirks();
                slf.emitter.emit_current_doctype();
                eof!()
            }
            c @ Some(_) => {
                error!(Error::MissingQuoteBeforeDoctypeSystemIdentifier);
                slf.emitter.set_force_quirks();
                reconsume_in!(c, State::BogusDoctype)
            }
        },
        State::AfterDoctypeSystemKeyword => match read_byte!()? {
            Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => {
                switch_to!(State::BeforeDoctypeSystemIdentifier)
            }
            Some(b'"') => {
                error!(Error::MissingWhitespaceAfterDoctypeSystemKeyword);
                slf.emitter.set_doctype_system_identifier(b"");
                switch_to!(State::DoctypeSystemIdentifierDoubleQuoted)
            }
            Some(b'\'') => {
                error!(Error::MissingWhitespaceAfterDoctypeSystemKeyword);
                slf.emitter.set_doctype_system_identifier(b"");
                switch_to!(State::DoctypeSystemIdentifierSingleQuoted)
            }
            Some(b'>') => {
                error!(Error::MissingDoctypeSystemIdentifier);
                slf.emitter.set_force_quirks();
                slf.emitter.emit_current_doctype();
                switch_to!(State::Data)
            }
            None => {
                error!(Error::EofInDoctype);
                slf.emitter.set_force_quirks();
                slf.emitter.emit_current_doctype();
                eof!()
            }
            c @ Some(_) => {
                error!(Error::MissingQuoteBeforeDoctypeSystemIdentifier);
                slf.emitter.set_force_quirks();
                reconsume_in!(c, State::BogusDoctype)
            }
        },
        State::BeforeDoctypeSystemIdentifier => match read_byte!()? {
            Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => cont!(),
            Some(b'"') => {
                slf.emitter.set_doctype_system_identifier(b"");
                switch_to!(State::DoctypeSystemIdentifierDoubleQuoted)
            }
            Some(b'\'') => {
                slf.emitter.set_doctype_system_identifier(b"");
                switch_to!(State::DoctypeSystemIdentifierSingleQuoted)
            }
            Some(b'>') => {
                error!(Error::MissingDoctypeSystemIdentifier);
                slf.emitter.set_force_quirks();
                slf.emitter.emit_current_doctype();
                switch_to!(State::Data)
            }
            None => {
                error!(Error::EofInDoctype);
                slf.emitter.set_force_quirks();
                slf.emitter.emit_current_doctype();
                eof!()
            }
            c @ Some(_) => {
                error!(Error::MissingQuoteBeforeDoctypeSystemIdentifier);
                slf.emitter.set_force_quirks();
                reconsume_in!(c, State::BogusDoctype)
            }
        },
        State::DoctypeSystemIdentifierDoubleQuoted => fast_read_char!(
            slf,
            match xs {
                Some(b"\"") => {
                    switch_to!(State::AfterDoctypeSystemIdentifier)
                }
                Some(b"\0") => {
                    error!(Error::UnexpectedNullCharacter);
                    slf.emitter
                        .push_doctype_system_identifier("\u{fffd}".as_bytes());
                    cont!()
                }
                Some(b">") => {
                    error!(Error::AbruptDoctypeSystemIdentifier);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    switch_to!(State::Data)
                }
                Some(xs) => {
                    slf.emitter.push_doctype_system_identifier(xs);
                    cont!()
                }
                None => {
                    error!(Error::EofInDoctype);
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
                    switch_to!(State::AfterDoctypeSystemIdentifier)
                }
                Some(b"\0") => {
                    error!(Error::UnexpectedNullCharacter);
                    slf.emitter
                        .push_doctype_system_identifier("\u{fffd}".as_bytes());
                    cont!()
                }
                Some(b">") => {
                    error!(Error::AbruptDoctypeSystemIdentifier);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    switch_to!(State::Data)
                }
                Some(xs) => {
                    slf.emitter.push_doctype_system_identifier(xs);
                    cont!()
                }
                None => {
                    error!(Error::EofInDoctype);
                    slf.emitter.set_force_quirks();
                    slf.emitter.emit_current_doctype();
                    eof!()
                }
            }
        ),
        State::AfterDoctypeSystemIdentifier => match read_byte!()? {
            Some(b'\t' | b'\x0A' | b'\x0C' | b' ') => cont!(),
            Some(b'>') => {
                slf.emitter.emit_current_doctype();
                switch_to!(State::Data)
            }
            None => {
                error!(Error::EofInDoctype);
                slf.emitter.set_force_quirks();
                slf.emitter.emit_current_doctype();
                eof!()
            }
            c @ Some(_) => {
                error!(Error::UnexpectedCharacterAfterDoctypeSystemIdentifier);
                reconsume_in!(c, State::BogusDoctype)
            }
        },
        State::BogusDoctype => fast_read_char!(
            slf,
            match xs {
                Some(b">") => {
                    slf.emitter.emit_current_doctype();
                    switch_to!(State::Data)
                }
                Some(b"\0") => {
                    error!(Error::UnexpectedNullCharacter);
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
                    switch_to!(State::CdataSectionBracket)
                }
                Some(xs) => {
                    slf.emitter.emit_string(xs);
                    cont!()
                }
                None => {
                    error!(Error::EofInCdata);
                    eof!()
                }
            }
        ),
        State::CdataSectionBracket => match read_byte!()? {
            Some(b']') => {
                switch_to!(State::CdataSectionEnd)
            }
            c => {
                slf.emitter.emit_string(b"]");
                reconsume_in!(c, State::CdataSection)
            }
        },
        State::CdataSectionEnd => match read_byte!()? {
            Some(b']') => {
                slf.emitter.emit_string(b"]");
                cont!()
            }
            Some(b'>') => {
                switch_to!(State::Data)
            }
            c => {
                slf.emitter.emit_string(b"]]");
                reconsume_in!(c, State::CdataSection)
            }
        },
        State::CharacterReference => {
            slf.machine_helper.temporary_buffer.clear();
            slf.machine_helper.temporary_buffer.push(b'&');

            match read_byte!()? {
                Some(x) if x.is_ascii_alphanumeric() => {
                    reconsume_in!(Some(x), State::NamedCharacterReference)
                }
                Some(b'#') => {
                    slf.machine_helper.temporary_buffer.push(b'#');
                    switch_to!(State::NumericCharacterReference)
                }
                c => {
                    slf.machine_helper
                        .flush_code_points_consumed_as_character_reference(&mut slf.emitter);
                    reconsume_in!(c, slf.machine_helper.pop_return_state())
                }
            }
        }
        State::NamedCharacterReference => {
            let c = read_byte!()?;

            let char_ref = match c {
                Some(x) => try_read_character_reference(x as char, |x| {
                    slf.reader.try_read_string(&mut slf.validator, x, true)
                })?
                .map(|char_ref| (x, char_ref)),

                None => None,
            };

            if let Some((x, char_ref)) = char_ref {
                let char_ref_name_last_character = char_ref.name.chars().last();
                let next_character = read_byte!()?;

                if !slf.machine_helper.is_consumed_as_part_of_an_attribute()
                    || char_ref_name_last_character == Some(';')
                    || !matches!(next_character, Some(x) if x == b'=' || x.is_ascii_alphanumeric())
                {
                    if char_ref_name_last_character != Some(';') {
                        error!(Error::MissingSemicolonAfterCharacterReference);
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
                reconsume_in!(next_character, slf.machine_helper.pop_return_state())
            } else {
                slf.machine_helper
                    .flush_code_points_consumed_as_character_reference(&mut slf.emitter);
                reconsume_in!(c, State::AmbiguousAmpersand)
            }
        }
        State::AmbiguousAmpersand => match read_byte!()? {
            Some(x) if x.is_ascii_alphanumeric() => {
                if slf.machine_helper.is_consumed_as_part_of_an_attribute() {
                    slf.emitter.push_attribute_value(&[x]);
                } else {
                    slf.emitter.emit_string(&[x]);
                }

                cont!()
            }
            c @ Some(b';') => {
                error!(Error::UnknownNamedCharacterReference);
                reconsume_in!(c, slf.machine_helper.pop_return_state())
            }
            c => {
                reconsume_in!(c, slf.machine_helper.pop_return_state())
            }
        },
        State::NumericCharacterReference => {
            slf.machine_helper.character_reference_code = 0;

            match read_byte!()? {
                Some(x @ b'x' | x @ b'X') => {
                    slf.machine_helper.temporary_buffer.push(x as u8);
                    switch_to!(State::HexadecimalCharacterReferenceStart)
                }
                Some(x @ b'0'..=b'9') => {
                    reconsume_in!(Some(x), State::DecimalCharacterReference)
                }
                c => {
                    error!(Error::AbsenceOfDigitsInNumericCharacterReference);
                    slf.machine_helper
                        .flush_code_points_consumed_as_character_reference(&mut slf.emitter);
                    reconsume_in!(c, slf.machine_helper.pop_return_state())
                }
            }
        }
        State::HexadecimalCharacterReferenceStart => match read_byte!()? {
            c @ Some(b'0'..=b'9' | b'A'..=b'F' | b'a'..=b'f') => {
                reconsume_in!(c, State::HexadecimalCharacterReference)
            }
            c => {
                error!(Error::AbsenceOfDigitsInNumericCharacterReference);
                slf.machine_helper
                    .flush_code_points_consumed_as_character_reference(&mut slf.emitter);
                reconsume_in!(c, slf.machine_helper.pop_return_state())
            }
        },
        State::HexadecimalCharacterReference => match read_byte!()? {
            Some(x @ b'0'..=b'9') => {
                mutate_character_reference!(*16 + x - 0x0030);
                cont!()
            }
            Some(x @ b'A'..=b'F') => {
                mutate_character_reference!(*16 + x - 0x0037);
                cont!()
            }
            Some(x @ b'a'..=b'f') => {
                mutate_character_reference!(*16 + x - 0x0057);
                cont!()
            }
            Some(b';') => {
                switch_to!(State::NumericCharacterReferenceEnd)
            }
            c => {
                error!(Error::MissingSemicolonAfterCharacterReference);
                reconsume_in!(c, State::NumericCharacterReferenceEnd)
            }
        },
        State::DecimalCharacterReference => match read_byte!()? {
            Some(x @ b'0'..=b'9') => {
                mutate_character_reference!(*10 + x - 0x0030);
                cont!()
            }
            Some(b';') => {
                switch_to!(State::NumericCharacterReferenceEnd)
            }
            c => {
                error!(Error::MissingSemicolonAfterCharacterReference);
                reconsume_in!(c, State::NumericCharacterReferenceEnd)
            }
        },
        State::NumericCharacterReferenceEnd => {
            match slf.machine_helper.character_reference_code {
                0x00 => {
                    error!(Error::NullCharacterReference);
                    slf.machine_helper.character_reference_code = 0xfffd;
                }
                0x110000.. => {
                    error!(Error::CharacterReferenceOutsideUnicodeRange);
                    slf.machine_helper.character_reference_code = 0xfffd;
                }
                surrogate_pat!() => {
                    error!(Error::SurrogateCharacterReference);
                    slf.machine_helper.character_reference_code = 0xfffd;
                }
                // noncharacter
                noncharacter_pat!() => {
                    error!(Error::NoncharacterCharacterReference);
                }
                // 0x000d, or a control that is not whitespace
                x @ 0x000d | x @ control_pat!()
                    if !matches!(x, 0x0009 | 0x000a | 0x000c | 0x0020) =>
                {
                    error!(Error::ControlCharacterReference);
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
            exit_state!()
        }
    }
}
