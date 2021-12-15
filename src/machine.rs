use crate::entities::try_read_character_reference;
use crate::read_helper::fast_read_char;
use crate::utils::{
    ascii_digit_pat, control_pat, ctostr, noncharacter_pat, surrogate_pat, whitespace_pat,
    with_lowercase_str, ControlToken, State,
};
use crate::{Emitter, Error, Reader, Tokenizer};

// Note: This is not implemented as a method on Tokenizer because there's fields on Tokenizer that
// should not be available in this method, such as Tokenizer.to_reconsume or the Reader instance
#[inline]
pub fn consume<R: Reader, E: Emitter>(slf: &mut Tokenizer<R, E>) -> Result<ControlToken, R::Error> {
    let machine_helper = &mut slf.machine_helper;

    macro_rules! mutate_character_reference {
        (* $mul:literal + $x:ident - $sub:literal) => {
            match machine_helper
                .character_reference_code
                .checked_mul($mul)
                .and_then(|cr| cr.checked_add($x as u32 - $sub))
            {
                Some(cr) => machine_helper.character_reference_code = cr,
                None => {
                    // provoke err
                    machine_helper.character_reference_code = 0x110000;
                }
            };
        };
    }

    macro_rules! reconsume_in {
        ($c:expr, $state:expr) => {{
            machine_helper.state = $state;
            slf.reader.unread_char($c);
            ControlToken::Continue
        }};
    }

    match machine_helper.state {
        State::Data => fast_read_char!(
            slf,
            emitter,
            machine_helper,
            match xs {
                Some("&") => {
                    machine_helper.enter_state(State::CharacterReference);
                    ControlToken::Continue
                }
                Some("<") => {
                    machine_helper.state = State::TagOpen;
                    ControlToken::Continue
                }
                Some("\0") => {
                    emitter.emit_error(Error::UnexpectedNullCharacter);
                    emitter.emit_string("\0");
                    ControlToken::Continue
                }
                Some(xs) => {
                    emitter.emit_string(xs);
                    ControlToken::Continue
                }
                None => {
                    ControlToken::Eof
                }
            }
        ),

        State::RcData => fast_read_char!(
            slf,
            emitter,
            machine_helper,
            match xs {
                Some("&") => {
                    machine_helper.enter_state(State::CharacterReference);
                    ControlToken::Continue
                }
                Some("<") => {
                    machine_helper.state = State::RcDataLessThanSign;
                    ControlToken::Continue
                }
                Some("\0") => {
                    emitter.emit_error(Error::UnexpectedNullCharacter);
                    emitter.emit_string("\u{fffd}");
                    ControlToken::Continue
                }
                Some(xs) => {
                    emitter.emit_string(xs);
                    ControlToken::Continue
                }
                None => {
                    ControlToken::Eof
                }
            }
        ),
        State::RawText => fast_read_char!(
            slf,
            emitter,
            machine_helper,
            match xs {
                Some("<") => {
                    machine_helper.state = State::RawTextLessThanSign;
                    ControlToken::Continue
                }
                Some("\0") => {
                    emitter.emit_error(Error::UnexpectedNullCharacter);
                    emitter.emit_string("\u{fffd}");
                    ControlToken::Continue
                }
                Some(xs) => {
                    emitter.emit_string(xs);
                    ControlToken::Continue
                }
                None => {
                    ControlToken::Eof
                }
            }
        ),
        State::ScriptData => fast_read_char!(
            slf,
            emitter,
            machine_helper,
            match xs {
                Some("<") => {
                    machine_helper.state = State::ScriptDataLessThanSign;
                    ControlToken::Continue
                }
                Some("\0") => {
                    emitter.emit_error(Error::UnexpectedNullCharacter);
                    emitter.emit_string("\u{fffd}");
                    ControlToken::Continue
                }
                Some(xs) => {
                    emitter.emit_string(xs);
                    ControlToken::Continue
                }
                None => {
                    ControlToken::Eof
                }
            }
        ),
        State::PlainText => fast_read_char!(
            slf,
            emitter,
            machine_helper,
            match xs {
                Some("\0") => {
                    emitter.emit_error(Error::UnexpectedNullCharacter);
                    emitter.emit_string("\u{fffd}");
                    ControlToken::Continue
                }
                Some(xs) => {
                    emitter.emit_string(xs);
                    ControlToken::Continue
                }
                None => {
                    ControlToken::Eof
                }
            }
        ),
        State::TagOpen => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some('!') => {
                    machine_helper.state = State::MarkupDeclarationOpen;
                    ControlToken::Continue
                }
                Some('/') => {
                    machine_helper.state = State::EndTagOpen;
                    ControlToken::Continue
                }
                Some(x) if x.is_ascii_alphabetic() => {
                    emitter.init_start_tag();
                    reconsume_in!(Some(x), State::TagName)
                }
                c @ Some('?') => {
                    emitter.emit_error(Error::UnexpectedQuestionMarkInsteadOfTagName);
                    emitter.init_comment();
                    reconsume_in!(c, State::BogusComment)
                }
                None => {
                    emitter.emit_error(Error::EofBeforeTagName);
                    emitter.emit_string("<");
                    ControlToken::Eof
                }
                c @ Some(_) => {
                    emitter.emit_error(Error::InvalidFirstCharacterOfTagName);
                    emitter.emit_string("<");
                    reconsume_in!(c, State::Data)
                }
            }
        }),
        State::EndTagOpen => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(x) if x.is_ascii_alphabetic() => {
                    emitter.init_end_tag();
                    reconsume_in!(Some(x), State::TagName)
                }
                Some('>') => {
                    emitter.emit_error(Error::MissingEndTagName);
                    machine_helper.state = State::Data;
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofBeforeTagName);
                    emitter.emit_string("</");
                    ControlToken::Eof
                }
                Some(x) => {
                    emitter.emit_error(Error::InvalidFirstCharacterOfTagName);
                    emitter.init_comment();
                    reconsume_in!(Some(x), State::BogusComment)
                }
            }
        }),
        State::TagName => fast_read_char!(
            slf,
            emitter,
            machine_helper,
            match xs {
                Some("\t" | "\u{0A}" | "\u{0C}" | " ") => {
                    machine_helper.state = State::BeforeAttributeName;
                    ControlToken::Continue
                }
                Some("/") => {
                    machine_helper.state = State::SelfClosingStartTag;
                    ControlToken::Continue
                }
                Some(">") => {
                    machine_helper.state = State::Data;
                    emitter.emit_current_tag();
                    ControlToken::Continue
                }
                Some("\0") => {
                    emitter.emit_error(Error::UnexpectedNullCharacter);
                    emitter.push_tag_name("\u{fffd}");
                    ControlToken::Continue
                }
                Some(xs) => {
                    with_lowercase_str(xs, |x| {
                        emitter.push_tag_name(x);
                    });

                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInTag);
                    ControlToken::Eof
                }
            }
        ),
        State::RcDataLessThanSign => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some('/') => {
                    machine_helper.temporary_buffer.clear();
                    machine_helper.state = State::RcDataEndTagOpen;
                    ControlToken::Continue
                }
                c => {
                    emitter.emit_string("<");
                    reconsume_in!(c, State::RcData)
                }
            }
        }),
        State::RcDataEndTagOpen => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(x) if x.is_ascii_alphabetic() => {
                    emitter.init_end_tag();
                    reconsume_in!(Some(x), State::RcDataEndTagName)
                }
                c => {
                    emitter.emit_string("</");
                    reconsume_in!(c, State::RcData)
                }
            }
        }),
        State::RcDataEndTagName => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(whitespace_pat!()) if emitter.current_is_appropriate_end_tag_token() => {
                    machine_helper.state = State::BeforeAttributeName;
                    ControlToken::Continue
                }
                Some('/') if emitter.current_is_appropriate_end_tag_token() => {
                    machine_helper.state = State::SelfClosingStartTag;
                    ControlToken::Continue
                }
                Some('>') if emitter.current_is_appropriate_end_tag_token() => {
                    machine_helper.state = State::Data;
                    emitter.emit_current_tag();
                    ControlToken::Continue
                }
                Some(x) if x.is_ascii_alphabetic() => {
                    emitter.push_tag_name(ctostr!(x.to_ascii_lowercase()));
                    machine_helper.temporary_buffer.push(x);
                    ControlToken::Continue
                }
                c => {
                    emitter.emit_string("</");
                    machine_helper.flush_buffer_characters(&mut slf.emitter);
                    reconsume_in!(c, State::RcData)
                }
            }
        }),
        State::RawTextLessThanSign => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some('/') => {
                    machine_helper.temporary_buffer.clear();
                    machine_helper.state = State::RawTextEndTagOpen;
                    ControlToken::Continue
                }
                c => {
                    emitter.emit_string("<");
                    reconsume_in!(c, State::RawText)
                }
            }
        }),
        State::RawTextEndTagOpen => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(x) if x.is_ascii_alphabetic() => {
                    emitter.init_end_tag();
                    reconsume_in!(Some(x), State::RawTextEndTagName)
                }
                c => {
                    emitter.emit_string("</");
                    reconsume_in!(c, State::RawText)
                }
            }
        }),
        State::RawTextEndTagName => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(whitespace_pat!()) if emitter.current_is_appropriate_end_tag_token() => {
                    machine_helper.state = State::BeforeAttributeName;
                    ControlToken::Continue
                }
                Some('/') if emitter.current_is_appropriate_end_tag_token() => {
                    machine_helper.state = State::SelfClosingStartTag;
                    ControlToken::Continue
                }
                Some('>') if emitter.current_is_appropriate_end_tag_token() => {
                    machine_helper.state = State::Data;
                    emitter.emit_current_tag();
                    ControlToken::Continue
                }
                Some(x) if x.is_ascii_alphabetic() => {
                    emitter.push_tag_name(ctostr!(x.to_ascii_lowercase()));
                    machine_helper.temporary_buffer.push(x);
                    ControlToken::Continue
                }
                c => {
                    emitter.emit_string("</");
                    machine_helper.flush_buffer_characters(&mut slf.emitter);
                    reconsume_in!(c, State::RawText)
                }
            }
        }),
        State::ScriptDataLessThanSign => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some('/') => {
                    machine_helper.temporary_buffer.clear();
                    machine_helper.state = State::ScriptDataEndTagOpen;
                    ControlToken::Continue
                }
                Some('!') => {
                    machine_helper.state = State::ScriptDataEscapeStart;
                    emitter.emit_string("<!");
                    ControlToken::Continue
                }
                c => {
                    emitter.emit_string("<");
                    reconsume_in!(c, State::ScriptData)
                }
            }
        }),
        State::ScriptDataEndTagOpen => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(x) if x.is_ascii_alphabetic() => {
                    emitter.init_end_tag();
                    reconsume_in!(Some(x), State::ScriptDataEndTagName)
                }
                c => {
                    emitter.emit_string("</");
                    reconsume_in!(c, State::ScriptData)
                }
            }
        }),
        State::ScriptDataEndTagName => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(whitespace_pat!()) if emitter.current_is_appropriate_end_tag_token() => {
                    machine_helper.state = State::BeforeAttributeName;
                    ControlToken::Continue
                }
                Some('/') if emitter.current_is_appropriate_end_tag_token() => {
                    machine_helper.state = State::SelfClosingStartTag;
                    ControlToken::Continue
                }
                Some('>') if emitter.current_is_appropriate_end_tag_token() => {
                    machine_helper.state = State::Data;
                    emitter.emit_current_tag();
                    ControlToken::Continue
                }
                Some(x) if x.is_ascii_alphabetic() => {
                    emitter.push_tag_name(ctostr!(x.to_ascii_lowercase()));
                    machine_helper.temporary_buffer.push(x.to_ascii_lowercase());
                    ControlToken::Continue
                }
                c => {
                    emitter.emit_string("</");
                    machine_helper.flush_buffer_characters(&mut slf.emitter);
                    reconsume_in!(c, State::Data)
                }
            }
        }),
        State::ScriptDataEscapeStart => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some('-') => {
                    machine_helper.state = State::ScriptDataEscapeStartDash;
                    emitter.emit_string("-");
                    ControlToken::Continue
                }
                c => {
                    reconsume_in!(c, State::ScriptData)
                }
            }
        }),
        State::ScriptDataEscapeStartDash => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some('-') => {
                    machine_helper.state = State::ScriptDataEscapedDashDash;
                    emitter.emit_string("-");
                    ControlToken::Continue
                }
                c => {
                    reconsume_in!(c, State::ScriptData)
                }
            }
        }),
        State::ScriptDataEscaped => fast_read_char!(
            slf,
            emitter,
            machine_helper,
            match xs {
                Some("-") => {
                    machine_helper.state = State::ScriptDataEscapedDash;
                    emitter.emit_string("-");
                    ControlToken::Continue
                }
                Some("<") => {
                    machine_helper.state = State::ScriptDataEscapedLessThanSign;
                    ControlToken::Continue
                }
                Some("\0") => {
                    emitter.emit_error(Error::UnexpectedNullCharacter);
                    emitter.emit_string("\u{fffd}");
                    ControlToken::Continue
                }
                Some(xs) => {
                    emitter.emit_string(xs);
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInScriptHtmlCommentLikeText);
                    ControlToken::Eof
                }
            }
        ),
        State::ScriptDataEscapedDash => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some('-') => {
                    machine_helper.state = State::ScriptDataEscapedDashDash;
                    emitter.emit_string("-");
                    ControlToken::Continue
                }
                Some('<') => {
                    machine_helper.state = State::ScriptDataEscapedLessThanSign;
                    ControlToken::Continue
                }
                Some('\0') => {
                    emitter.emit_error(Error::UnexpectedNullCharacter);
                    machine_helper.state = State::ScriptDataEscaped;
                    emitter.emit_string("\u{fffd}");
                    ControlToken::Continue
                }
                Some(x) => {
                    machine_helper.state = State::ScriptDataEscaped;
                    emitter.emit_string(ctostr!(x));
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInScriptHtmlCommentLikeText);
                    ControlToken::Eof
                }
            }
        }),
        State::ScriptDataEscapedDashDash => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some('-') => {
                    emitter.emit_string("-");
                    ControlToken::Continue
                }
                Some('<') => {
                    machine_helper.state = State::ScriptDataEscapedLessThanSign;
                    ControlToken::Continue
                }
                Some('>') => {
                    machine_helper.state = State::ScriptData;
                    emitter.emit_string(">");
                    ControlToken::Continue
                }
                Some('\0') => {
                    emitter.emit_error(Error::UnexpectedNullCharacter);
                    machine_helper.state = State::ScriptDataEscaped;
                    emitter.emit_string("\u{fffd}");
                    ControlToken::Continue
                }
                Some(x) => {
                    machine_helper.state = State::ScriptDataEscaped;
                    emitter.emit_string(ctostr!(x));
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInScriptHtmlCommentLikeText);
                    ControlToken::Eof
                }
            }
        }),
        State::ScriptDataEscapedLessThanSign => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some('/') => {
                    machine_helper.temporary_buffer.clear();
                    machine_helper.state = State::ScriptDataEscapedEndTagOpen;
                    ControlToken::Continue
                }
                Some(x) if x.is_ascii_alphabetic() => {
                    machine_helper.temporary_buffer.clear();
                    emitter.emit_string("<");
                    reconsume_in!(Some(x), State::ScriptDataDoubleEscapeStart)
                }
                c => {
                    emitter.emit_string("<");
                    reconsume_in!(c, State::ScriptDataEscaped)
                }
            }
        }),
        State::ScriptDataEscapedEndTagOpen => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(x) if x.is_ascii_alphabetic() => {
                    emitter.init_end_tag();
                    reconsume_in!(Some(x), State::ScriptDataEscapedEndTagName)
                }
                c => {
                    emitter.emit_string("</");
                    reconsume_in!(c, State::ScriptDataEscaped)
                }
            }
        }),
        State::ScriptDataEscapedEndTagName => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(whitespace_pat!()) if emitter.current_is_appropriate_end_tag_token() => {
                    machine_helper.state = State::BeforeAttributeName;
                    ControlToken::Continue
                }
                Some('/') if emitter.current_is_appropriate_end_tag_token() => {
                    machine_helper.state = State::SelfClosingStartTag;
                    ControlToken::Continue
                }
                Some('>') if emitter.current_is_appropriate_end_tag_token() => {
                    machine_helper.state = State::Data;
                    emitter.emit_current_tag();
                    ControlToken::Continue
                }
                Some(x) if x.is_ascii_alphabetic() => {
                    emitter.push_tag_name(ctostr!(x.to_ascii_lowercase()));
                    machine_helper.temporary_buffer.push(x);
                    ControlToken::Continue
                }
                c => {
                    emitter.emit_string("</");
                    machine_helper.flush_buffer_characters(&mut slf.emitter);
                    reconsume_in!(c, State::ScriptDataEscaped)
                }
            }
        }),
        State::ScriptDataDoubleEscapeStart => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(x @ whitespace_pat!() | x @ '/' | x @ '>') => {
                    if machine_helper.temporary_buffer == "script" {
                        machine_helper.state = State::ScriptDataDoubleEscaped;
                    } else {
                        machine_helper.state = State::ScriptDataEscaped;
                    }
                    emitter.emit_string(ctostr!(x));
                    ControlToken::Continue
                }
                Some(x) if x.is_ascii_alphabetic() => {
                    machine_helper.temporary_buffer.push(x.to_ascii_lowercase());
                    emitter.emit_string(ctostr!(x));
                    ControlToken::Continue
                }
                c => {
                    reconsume_in!(c, State::ScriptDataEscaped)
                }
            }
        }),
        State::ScriptDataDoubleEscaped => fast_read_char!(
            slf,
            emitter,
            machine_helper,
            match xs {
                Some("-") => {
                    machine_helper.state = State::ScriptDataDoubleEscapedDash;
                    emitter.emit_string("-");
                    ControlToken::Continue
                }
                Some("<") => {
                    machine_helper.state = State::ScriptDataDoubleEscapedLessThanSign;
                    emitter.emit_string("<");
                    ControlToken::Continue
                }
                Some("\0") => {
                    emitter.emit_error(Error::UnexpectedNullCharacter);
                    emitter.emit_string("\u{fffd}");
                    ControlToken::Continue
                }
                Some(xs) => {
                    emitter.emit_string(xs);
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInScriptHtmlCommentLikeText);
                    ControlToken::Eof
                }
            }
        ),
        State::ScriptDataDoubleEscapedDash => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some('-') => {
                    machine_helper.state = State::ScriptDataDoubleEscapedDashDash;
                    emitter.emit_string("-");
                    ControlToken::Continue
                }
                Some('<') => {
                    machine_helper.state = State::ScriptDataDoubleEscapedLessThanSign;
                    emitter.emit_string("<");
                    ControlToken::Continue
                }
                Some('\0') => {
                    emitter.emit_error(Error::UnexpectedNullCharacter);
                    machine_helper.state = State::ScriptDataDoubleEscaped;
                    emitter.emit_string("\u{fffd}");
                    ControlToken::Continue
                }
                Some(x) => {
                    machine_helper.state = State::ScriptDataDoubleEscaped;
                    emitter.emit_string(ctostr!(x));
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInScriptHtmlCommentLikeText);
                    ControlToken::Eof
                }
            }
        }),
        State::ScriptDataDoubleEscapedDashDash => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some('-') => {
                    emitter.emit_string("-");
                    ControlToken::Continue
                }
                Some('<') => {
                    emitter.emit_string("<");
                    machine_helper.state = State::ScriptDataDoubleEscapedLessThanSign;
                    ControlToken::Continue
                }
                Some('>') => {
                    emitter.emit_string(">");
                    machine_helper.state = State::ScriptData;
                    ControlToken::Continue
                }
                Some('\0') => {
                    emitter.emit_error(Error::UnexpectedNullCharacter);
                    machine_helper.state = State::ScriptDataDoubleEscaped;
                    emitter.emit_string("\u{fffd}");
                    ControlToken::Continue
                }
                Some(x) => {
                    machine_helper.state = State::ScriptDataDoubleEscaped;
                    emitter.emit_string(ctostr!(x));
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInScriptHtmlCommentLikeText);
                    ControlToken::Eof
                }
            }
        }),
        State::ScriptDataDoubleEscapedLessThanSign => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some('/') => {
                    machine_helper.temporary_buffer.clear();
                    machine_helper.state = State::ScriptDataDoubleEscapeEnd;
                    emitter.emit_string("/");
                    ControlToken::Continue
                }
                c => {
                    reconsume_in!(c, State::ScriptDataDoubleEscaped)
                }
            }
        }),
        State::ScriptDataDoubleEscapeEnd => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(x @ whitespace_pat!() | x @ '/' | x @ '>') => {
                    if machine_helper.temporary_buffer == "script" {
                        machine_helper.state = State::ScriptDataEscaped;
                    } else {
                        machine_helper.state = State::ScriptDataDoubleEscaped;
                    }

                    emitter.emit_string(ctostr!(x));
                    ControlToken::Continue
                }
                Some(x) if x.is_ascii_alphabetic() => {
                    machine_helper.temporary_buffer.push(x.to_ascii_lowercase());
                    emitter.emit_string(ctostr!(x));
                    ControlToken::Continue
                }
                c => {
                    reconsume_in!(c, State::ScriptDataDoubleEscaped)
                }
            }
        }),
        State::BeforeAttributeName => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(whitespace_pat!()) => ControlToken::Continue,
                c @ Some('/' | '>') | c @ None => {
                    reconsume_in!(c, State::AfterAttributeName)
                }
                Some('=') => {
                    emitter.emit_error(Error::UnexpectedEqualsSignBeforeAttributeName);
                    emitter.init_attribute();
                    emitter.push_attribute_name("=");
                    machine_helper.state = State::AttributeName;
                    ControlToken::Continue
                }
                Some(x) => {
                    emitter.init_attribute();
                    reconsume_in!(Some(x), State::AttributeName)
                }
            }
        }),
        State::AttributeName => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                c @ Some(whitespace_pat!() | '/' | '>') | c @ None => {
                    reconsume_in!(c, State::AfterAttributeName)
                }
                Some('=') => {
                    machine_helper.state = State::BeforeAttributeValue;
                    ControlToken::Continue
                }
                Some('\0') => {
                    emitter.emit_error(Error::UnexpectedNullCharacter);
                    emitter.push_attribute_name("\u{fffd}");
                    ControlToken::Continue
                }
                Some(x @ '"' | x @ '\'' | x @ '<') => {
                    emitter.emit_error(Error::UnexpectedCharacterInAttributeName);
                    emitter.push_attribute_name(ctostr!(x.to_ascii_lowercase()));
                    ControlToken::Continue
                }
                Some(x) => {
                    emitter.push_attribute_name(ctostr!(x.to_ascii_lowercase()));
                    ControlToken::Continue
                }
            }
        }),
        State::AfterAttributeName => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(whitespace_pat!()) => ControlToken::Continue,
                Some('/') => {
                    machine_helper.state = State::SelfClosingStartTag;
                    ControlToken::Continue
                }
                Some('=') => {
                    machine_helper.state = State::BeforeAttributeValue;
                    ControlToken::Continue
                }
                Some('>') => {
                    machine_helper.state = State::Data;
                    emitter.emit_current_tag();
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInTag);
                    ControlToken::Eof
                }
                Some(x) => {
                    emitter.init_attribute();
                    reconsume_in!(Some(x), State::AttributeName)
                }
            }
        }),
        State::BeforeAttributeValue => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(whitespace_pat!()) => ControlToken::Continue,
                Some('"') => {
                    machine_helper.state = State::AttributeValueDoubleQuoted;
                    ControlToken::Continue
                }
                Some('\'') => {
                    machine_helper.state = State::AttributeValueSingleQuoted;
                    ControlToken::Continue
                }
                Some('>') => {
                    emitter.emit_error(Error::MissingAttributeValue);
                    machine_helper.state = State::Data;
                    emitter.emit_current_tag();
                    ControlToken::Continue
                }
                c => {
                    reconsume_in!(c, State::AttributeValueUnquoted)
                }
            }
        }),
        State::AttributeValueDoubleQuoted => fast_read_char!(
            slf,
            emitter,
            machine_helper,
            match xs {
                Some("\"") => {
                    machine_helper.state = State::AfterAttributeValueQuoted;
                    ControlToken::Continue
                }
                Some("&") => {
                    machine_helper.enter_state(State::CharacterReference);
                    ControlToken::Continue
                }
                Some("\0") => {
                    emitter.emit_error(Error::UnexpectedNullCharacter);
                    emitter.push_attribute_value("\u{fffd}");
                    ControlToken::Continue
                }
                Some(xs) => {
                    emitter.push_attribute_value(xs);
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInTag);
                    ControlToken::Eof
                }
            }
        ),
        State::AttributeValueSingleQuoted => fast_read_char!(
            slf,
            emitter,
            machine_helper,
            match xs {
                Some("'") => {
                    machine_helper.state = State::AfterAttributeValueQuoted;
                    ControlToken::Continue
                }
                Some("&") => {
                    machine_helper.enter_state(State::CharacterReference);
                    ControlToken::Continue
                }
                Some("\0") => {
                    emitter.emit_error(Error::UnexpectedNullCharacter);
                    emitter.push_attribute_value("\u{fffd}");
                    ControlToken::Continue
                }
                Some(xs) => {
                    emitter.push_attribute_value(xs);
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInTag);
                    ControlToken::Eof
                }
            }
        ),
        State::AttributeValueUnquoted => fast_read_char!(
            slf,
            emitter,
            machine_helper,
            match xs {
                Some("\t" | "\u{0A}" | "\u{0C}" | " ") => {
                    machine_helper.state = State::BeforeAttributeName;
                    ControlToken::Continue
                }
                Some("&") => {
                    machine_helper.enter_state(State::CharacterReference);
                    ControlToken::Continue
                }
                Some(">") => {
                    machine_helper.state = State::Data;
                    emitter.emit_current_tag();
                    ControlToken::Continue
                }
                Some("\0") => {
                    emitter.emit_error(Error::UnexpectedNullCharacter);
                    emitter.push_attribute_value("\u{fffd}");
                    ControlToken::Continue
                }
                Some("\"" | "'" | "<" | "=" | "\u{60}") => {
                    emitter.emit_error(Error::UnexpectedCharacterInUnquotedAttributeValue);
                    emitter.push_attribute_value(xs.unwrap());
                    ControlToken::Continue
                }
                Some(xs) => {
                    emitter.push_attribute_value(xs);
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInTag);
                    ControlToken::Eof
                }
            }
        ),
        State::AfterAttributeValueQuoted => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(whitespace_pat!()) => {
                    machine_helper.state = State::BeforeAttributeName;
                    ControlToken::Continue
                }
                Some('/') => {
                    machine_helper.state = State::SelfClosingStartTag;
                    ControlToken::Continue
                }
                Some('>') => {
                    machine_helper.state = State::Data;
                    emitter.emit_current_tag();
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInTag);
                    ControlToken::Eof
                }
                Some(x) => {
                    emitter.emit_error(Error::MissingWhitespaceBetweenAttributes);
                    reconsume_in!(Some(x), State::BeforeAttributeName)
                }
            }
        }),
        State::SelfClosingStartTag => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some('>') => {
                    emitter.set_self_closing();
                    machine_helper.state = State::Data;
                    emitter.emit_current_tag();
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInTag);
                    ControlToken::Eof
                }
                Some(x) => {
                    emitter.emit_error(Error::UnexpectedSolidusInTag);
                    reconsume_in!(Some(x), State::BeforeAttributeName)
                }
            }
        }),
        State::BogusComment => fast_read_char!(
            slf,
            emitter,
            machine_helper,
            match xs {
                Some(">") => {
                    machine_helper.state = State::Data;
                    emitter.emit_current_comment();
                    ControlToken::Continue
                }
                Some("\0") => {
                    emitter.emit_error(Error::UnexpectedNullCharacter);
                    emitter.push_comment("\u{fffd}");
                    ControlToken::Continue
                }
                Some(xs) => {
                    emitter.push_comment(xs);
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_current_comment();
                    ControlToken::Eof
                }
            }
        ),
        State::MarkupDeclarationOpen => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some('-') if slf.reader.try_read_string("-", true)? => {
                    emitter.init_comment();
                    machine_helper.state = State::CommentStart;
                    ControlToken::Continue
                }
                Some('d' | 'D') if slf.reader.try_read_string("octype", false)? => {
                    machine_helper.state = State::Doctype;
                    ControlToken::Continue
                }
                Some('[') if slf.reader.try_read_string("CDATA[", true)? => {
                    // missing: check for adjusted current element: we don't have an element stack
                    // at all
                    //
                    // missing: cdata transition
                    //
                    // let's hope that bogus comment can just sort of skip over cdata
                    emitter.emit_error(Error::CdataInHtmlContent);

                    emitter.init_comment();
                    emitter.push_comment("[CDATA[");
                    machine_helper.state = State::BogusComment;
                    ControlToken::Continue
                }
                c => {
                    emitter.emit_error(Error::IncorrectlyOpenedComment);
                    emitter.init_comment();
                    reconsume_in!(c, State::BogusComment)
                }
            }
        }),
        State::CommentStart => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some('-') => {
                    machine_helper.state = State::CommentStartDash;
                    ControlToken::Continue
                }
                Some('>') => {
                    emitter.emit_error(Error::AbruptClosingOfEmptyComment);
                    machine_helper.state = State::Data;
                    emitter.emit_current_comment();
                    ControlToken::Continue
                }
                c => {
                    reconsume_in!(c, State::Comment)
                }
            }
        }),
        State::CommentStartDash => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some('-') => {
                    machine_helper.state = State::CommentEnd;
                    ControlToken::Continue
                }
                Some('>') => {
                    emitter.emit_error(Error::AbruptClosingOfEmptyComment);
                    machine_helper.state = State::Data;
                    emitter.emit_current_comment();
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInComment);
                    emitter.emit_current_comment();
                    ControlToken::Eof
                }
                c @ Some(_) => {
                    emitter.push_comment("-");
                    reconsume_in!(c, State::Comment)
                }
            }
        }),
        State::Comment => fast_read_char!(
            slf,
            emitter,
            machine_helper,
            match xs {
                Some("<") => {
                    emitter.push_comment("<");
                    machine_helper.state = State::CommentLessThanSign;
                    ControlToken::Continue
                }
                Some("-") => {
                    machine_helper.state = State::CommentEndDash;
                    ControlToken::Continue
                }
                Some("\0") => {
                    emitter.emit_error(Error::UnexpectedNullCharacter);
                    emitter.push_comment("\u{fffd}");
                    ControlToken::Continue
                }
                Some(xs) => {
                    emitter.push_comment(xs);
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInComment);
                    emitter.emit_current_comment();
                    ControlToken::Eof
                }
            }
        ),
        State::CommentLessThanSign => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some('!') => {
                    emitter.push_comment("!");
                    machine_helper.state = State::CommentLessThanSignBang;
                    ControlToken::Continue
                }
                Some('<') => {
                    emitter.push_comment("<");
                    ControlToken::Continue
                }
                c => {
                    reconsume_in!(c, State::Comment)
                }
            }
        }),
        State::CommentLessThanSignBang => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some('-') => {
                    machine_helper.state = State::CommentLessThanSignBangDash;
                    ControlToken::Continue
                }
                c => {
                    reconsume_in!(c, State::Comment)
                }
            }
        }),
        State::CommentLessThanSignBangDash => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some('-') => {
                    machine_helper.state = State::CommentLessThanSignBangDashDash;
                    ControlToken::Continue
                }
                c => {
                    reconsume_in!(c, State::CommentEndDash)
                }
            }
        }),
        State::CommentLessThanSignBangDashDash => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                c @ Some('>') | c @ None => {
                    reconsume_in!(c, State::CommentEnd)
                }
                c => {
                    emitter.emit_error(Error::NestedComment);
                    reconsume_in!(c, State::CommentEnd)
                }
            }
        }),
        State::CommentEndDash => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some('-') => {
                    machine_helper.state = State::CommentEnd;
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInComment);
                    emitter.emit_current_comment();
                    ControlToken::Eof
                }
                c => {
                    emitter.push_comment("-");
                    reconsume_in!(c, State::Comment)
                }
            }
        }),
        State::CommentEnd => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some('>') => {
                    machine_helper.state = State::Data;
                    emitter.emit_current_comment();
                    ControlToken::Continue
                }
                Some('!') => {
                    machine_helper.state = State::CommentEndBang;
                    ControlToken::Continue
                }
                Some('-') => {
                    emitter.push_comment("-");
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInComment);
                    emitter.emit_current_comment();
                    ControlToken::Eof
                }
                c @ Some(_) => {
                    emitter.push_comment("-");
                    emitter.push_comment("-");
                    reconsume_in!(c, State::Comment)
                }
            }
        }),
        State::CommentEndBang => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some('-') => {
                    emitter.push_comment("-");
                    emitter.push_comment("-");
                    emitter.push_comment("!");
                    machine_helper.state = State::CommentEndDash;
                    ControlToken::Continue
                }
                Some('>') => {
                    emitter.emit_error(Error::IncorrectlyClosedComment);
                    machine_helper.state = State::Data;
                    emitter.emit_current_comment();
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInComment);
                    emitter.emit_current_comment();
                    ControlToken::Eof
                }
                c @ Some(_) => {
                    emitter.push_comment("-");
                    emitter.push_comment("-");
                    emitter.push_comment("!");
                    reconsume_in!(c, State::Comment)
                }
            }
        }),
        State::Doctype => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(whitespace_pat!()) => {
                    machine_helper.state = State::BeforeDoctypeName;
                    ControlToken::Continue
                }
                c @ Some('>') => {
                    reconsume_in!(c, State::BeforeDoctypeName)
                }
                None => {
                    emitter.emit_error(Error::EofInDoctype);
                    emitter.init_doctype();
                    emitter.set_force_quirks();
                    emitter.emit_current_doctype();
                    ControlToken::Eof
                }
                c @ Some(_) => {
                    emitter.emit_error(Error::MissingWhitespaceBeforeDoctypeName);
                    reconsume_in!(c, State::BeforeDoctypeName)
                }
            }
        }),
        State::BeforeDoctypeName => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(whitespace_pat!()) => ControlToken::Continue,
                Some('\0') => {
                    emitter.emit_error(Error::UnexpectedNullCharacter);
                    emitter.init_doctype();
                    emitter.push_doctype_name("\u{fffd}");
                    machine_helper.state = State::DoctypeName;
                    ControlToken::Continue
                }
                Some('>') => {
                    emitter.emit_error(Error::MissingDoctypeName);
                    emitter.init_doctype();
                    emitter.set_force_quirks();
                    machine_helper.state = State::Data;
                    emitter.emit_current_doctype();
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInDoctype);
                    emitter.init_doctype();
                    emitter.set_force_quirks();
                    emitter.emit_current_doctype();
                    ControlToken::Eof
                }
                Some(x) => {
                    emitter.init_doctype();
                    emitter.push_doctype_name(ctostr!(x.to_ascii_lowercase()));
                    machine_helper.state = State::DoctypeName;
                    ControlToken::Continue
                }
            }
        }),
        State::DoctypeName => fast_read_char!(
            slf,
            emitter,
            machine_helper,
            match xs {
                Some("\t" | "\u{0A}" | "\u{0C}" | " ") => {
                    machine_helper.state = State::AfterDoctypeName;
                    ControlToken::Continue
                }
                Some(">") => {
                    machine_helper.state = State::Data;
                    emitter.emit_current_doctype();
                    ControlToken::Continue
                }
                Some("\0") => {
                    emitter.emit_error(Error::UnexpectedNullCharacter);
                    emitter.push_doctype_name("\u{fffd}");
                    ControlToken::Continue
                }
                Some(xs) => {
                    with_lowercase_str(xs, |x| {
                        emitter.push_doctype_name(x);
                    });
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInDoctype);
                    emitter.set_force_quirks();
                    emitter.emit_current_doctype();
                    ControlToken::Eof
                }
            }
        ),
        State::AfterDoctypeName => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(whitespace_pat!()) => ControlToken::Continue,
                Some('>') => {
                    machine_helper.state = State::Data;
                    emitter.emit_current_doctype();
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInDoctype);
                    emitter.set_force_quirks();
                    emitter.emit_current_doctype();
                    ControlToken::Eof
                }
                Some('p' | 'P') if slf.reader.try_read_string("ublic", false)? => {
                    machine_helper.state = State::AfterDoctypePublicKeyword;
                    ControlToken::Continue
                }
                Some('s' | 'S') if slf.reader.try_read_string("ystem", false)? => {
                    machine_helper.state = State::AfterDoctypeSystemKeyword;
                    ControlToken::Continue
                }
                c @ Some(_) => {
                    emitter.emit_error(Error::InvalidCharacterSequenceAfterDoctypeName);
                    emitter.set_force_quirks();
                    reconsume_in!(c, State::BogusDoctype)
                }
            }
        }),
        State::AfterDoctypePublicKeyword => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(whitespace_pat!()) => {
                    machine_helper.state = State::BeforeDoctypePublicIdentifier;
                    ControlToken::Continue
                }
                Some('"') => {
                    emitter.emit_error(Error::MissingWhitespaceAfterDoctypePublicKeyword);
                    emitter.set_doctype_public_identifier("");
                    machine_helper.state = State::DoctypePublicIdentifierDoubleQuoted;
                    ControlToken::Continue
                }
                Some('\'') => {
                    emitter.emit_error(Error::MissingWhitespaceAfterDoctypePublicKeyword);
                    emitter.set_doctype_public_identifier("");
                    machine_helper.state = State::DoctypePublicIdentifierSingleQuoted;
                    ControlToken::Continue
                }
                Some('>') => {
                    emitter.emit_error(Error::MissingDoctypePublicIdentifier);
                    emitter.set_force_quirks();
                    machine_helper.state = State::Data;
                    emitter.emit_current_doctype();
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInDoctype);
                    emitter.set_force_quirks();
                    emitter.emit_current_doctype();
                    ControlToken::Eof
                }
                c @ Some(_) => {
                    emitter.emit_error(Error::MissingQuoteBeforeDoctypePublicIdentifier);
                    emitter.set_force_quirks();
                    reconsume_in!(c, State::BogusDoctype)
                }
            }
        }),
        State::BeforeDoctypePublicIdentifier => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(whitespace_pat!()) => ControlToken::Continue,
                Some('"') => {
                    emitter.set_doctype_public_identifier("");
                    machine_helper.state = State::DoctypePublicIdentifierDoubleQuoted;
                    ControlToken::Continue
                }
                Some('\'') => {
                    emitter.set_doctype_public_identifier("");
                    machine_helper.state = State::DoctypePublicIdentifierSingleQuoted;
                    ControlToken::Continue
                }
                Some('>') => {
                    emitter.emit_error(Error::MissingDoctypePublicIdentifier);
                    emitter.set_force_quirks();
                    machine_helper.state = State::Data;
                    emitter.emit_current_doctype();
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInDoctype);
                    emitter.set_force_quirks();
                    emitter.emit_current_doctype();
                    ControlToken::Eof
                }
                c @ Some(_) => {
                    emitter.emit_error(Error::MissingQuoteBeforeDoctypePublicIdentifier);
                    emitter.set_force_quirks();
                    reconsume_in!(c, State::BogusDoctype)
                }
            }
        }),
        State::DoctypePublicIdentifierDoubleQuoted => fast_read_char!(
            slf,
            emitter,
            machine_helper,
            match xs {
                Some("\"") => {
                    machine_helper.state = State::AfterDoctypePublicIdentifier;
                    ControlToken::Continue
                }
                Some("\0") => {
                    emitter.emit_error(Error::UnexpectedNullCharacter);
                    emitter.push_doctype_public_identifier("\u{fffd}");
                    ControlToken::Continue
                }
                Some(">") => {
                    emitter.emit_error(Error::AbruptDoctypePublicIdentifier);
                    emitter.set_force_quirks();
                    machine_helper.state = State::Data;
                    emitter.emit_current_doctype();
                    ControlToken::Continue
                }
                Some(xs) => {
                    emitter.push_doctype_public_identifier(xs);
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInDoctype);
                    emitter.set_force_quirks();
                    emitter.emit_current_doctype();
                    ControlToken::Eof
                }
            }
        ),
        State::DoctypePublicIdentifierSingleQuoted => fast_read_char!(
            slf,
            emitter,
            machine_helper,
            match xs {
                Some("'") => {
                    machine_helper.state = State::AfterDoctypePublicIdentifier;
                    ControlToken::Continue
                }
                Some("\0") => {
                    emitter.emit_error(Error::UnexpectedNullCharacter);
                    emitter.push_doctype_public_identifier("\u{fffd}");
                    ControlToken::Continue
                }
                Some(">") => {
                    emitter.emit_error(Error::AbruptDoctypePublicIdentifier);
                    emitter.set_force_quirks();
                    machine_helper.state = State::Data;
                    emitter.emit_current_doctype();
                    ControlToken::Continue
                }
                Some(xs) => {
                    emitter.push_doctype_public_identifier(xs);
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInDoctype);
                    emitter.set_force_quirks();
                    emitter.emit_current_doctype();
                    ControlToken::Eof
                }
            }
        ),
        State::AfterDoctypePublicIdentifier => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(whitespace_pat!()) => {
                    machine_helper.state = State::BetweenDoctypePublicAndSystemIdentifiers;
                    ControlToken::Continue
                }
                Some('>') => {
                    machine_helper.state = State::Data;
                    emitter.emit_current_doctype();
                    ControlToken::Continue
                }
                Some('"') => {
                    emitter.emit_error(
                        Error::MissingWhitespaceBetweenDoctypePublicAndSystemIdentifiers,
                    );
                    emitter.set_doctype_system_identifier("");
                    machine_helper.state = State::DoctypeSystemIdentifierDoubleQuoted;
                    ControlToken::Continue
                }
                Some('\'') => {
                    emitter.emit_error(
                        Error::MissingWhitespaceBetweenDoctypePublicAndSystemIdentifiers,
                    );
                    emitter.set_doctype_system_identifier("");
                    machine_helper.state = State::DoctypeSystemIdentifierSingleQuoted;
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInDoctype);
                    emitter.set_force_quirks();
                    emitter.emit_current_doctype();
                    ControlToken::Eof
                }
                c @ Some(_) => {
                    emitter.emit_error(Error::MissingQuoteBeforeDoctypeSystemIdentifier);
                    emitter.set_force_quirks();
                    reconsume_in!(c, State::BogusDoctype)
                }
            }
        }),
        State::BetweenDoctypePublicAndSystemIdentifiers => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(whitespace_pat!()) => ControlToken::Continue,
                Some('>') => {
                    machine_helper.state = State::Data;
                    emitter.emit_current_doctype();
                    ControlToken::Continue
                }
                Some('"') => {
                    emitter.set_doctype_system_identifier("");
                    machine_helper.state = State::DoctypeSystemIdentifierDoubleQuoted;
                    ControlToken::Continue
                }
                Some('\'') => {
                    emitter.set_doctype_system_identifier("");
                    machine_helper.state = State::DoctypeSystemIdentifierSingleQuoted;
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInDoctype);
                    emitter.set_force_quirks();
                    emitter.emit_current_doctype();
                    ControlToken::Eof
                }
                c @ Some(_) => {
                    emitter.emit_error(Error::MissingQuoteBeforeDoctypeSystemIdentifier);
                    emitter.set_force_quirks();
                    reconsume_in!(c, State::BogusDoctype)
                }
            }
        }),
        State::AfterDoctypeSystemKeyword => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(whitespace_pat!()) => {
                    machine_helper.state = State::BeforeDoctypeSystemIdentifier;
                    ControlToken::Continue
                }
                Some('"') => {
                    emitter.emit_error(Error::MissingWhitespaceAfterDoctypeSystemKeyword);
                    emitter.set_doctype_system_identifier("");
                    machine_helper.state = State::DoctypeSystemIdentifierDoubleQuoted;
                    ControlToken::Continue
                }
                Some('\'') => {
                    emitter.emit_error(Error::MissingWhitespaceAfterDoctypeSystemKeyword);
                    emitter.set_doctype_system_identifier("");
                    machine_helper.state = State::DoctypeSystemIdentifierSingleQuoted;
                    ControlToken::Continue
                }
                Some('>') => {
                    emitter.emit_error(Error::MissingDoctypeSystemIdentifier);
                    emitter.set_force_quirks();
                    machine_helper.state = State::Data;
                    emitter.emit_current_doctype();
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInDoctype);
                    emitter.set_force_quirks();
                    emitter.emit_current_doctype();
                    ControlToken::Eof
                }
                c @ Some(_) => {
                    emitter.emit_error(Error::MissingQuoteBeforeDoctypeSystemIdentifier);
                    emitter.set_force_quirks();
                    reconsume_in!(c, State::BogusDoctype)
                }
            }
        }),
        State::BeforeDoctypeSystemIdentifier => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(whitespace_pat!()) => ControlToken::Continue,
                Some('"') => {
                    emitter.set_doctype_system_identifier("");
                    machine_helper.state = State::DoctypeSystemIdentifierDoubleQuoted;
                    ControlToken::Continue
                }
                Some('\'') => {
                    emitter.set_doctype_system_identifier("");
                    machine_helper.state = State::DoctypeSystemIdentifierSingleQuoted;
                    ControlToken::Continue
                }
                Some('>') => {
                    emitter.emit_error(Error::MissingDoctypeSystemIdentifier);
                    emitter.set_force_quirks();
                    machine_helper.state = State::Data;
                    emitter.emit_current_doctype();
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInDoctype);
                    emitter.set_force_quirks();
                    emitter.emit_current_doctype();
                    ControlToken::Eof
                }
                c @ Some(_) => {
                    emitter.emit_error(Error::MissingQuoteBeforeDoctypeSystemIdentifier);
                    emitter.set_force_quirks();
                    reconsume_in!(c, State::BogusDoctype)
                }
            }
        }),
        State::DoctypeSystemIdentifierDoubleQuoted => fast_read_char!(
            slf,
            emitter,
            machine_helper,
            match xs {
                Some("\"") => {
                    machine_helper.state = State::AfterDoctypeSystemIdentifier;
                    ControlToken::Continue
                }
                Some("\0") => {
                    emitter.emit_error(Error::UnexpectedNullCharacter);
                    emitter.push_doctype_system_identifier("\u{fffd}");
                    ControlToken::Continue
                }
                Some(">") => {
                    emitter.emit_error(Error::AbruptDoctypeSystemIdentifier);
                    emitter.set_force_quirks();
                    machine_helper.state = State::Data;
                    emitter.emit_current_doctype();
                    ControlToken::Continue
                }
                Some(xs) => {
                    emitter.push_doctype_system_identifier(xs);
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInDoctype);
                    emitter.set_force_quirks();
                    emitter.emit_current_doctype();
                    ControlToken::Eof
                }
            }
        ),
        State::DoctypeSystemIdentifierSingleQuoted => fast_read_char!(
            slf,
            emitter,
            machine_helper,
            match xs {
                Some("\'") => {
                    machine_helper.state = State::AfterDoctypeSystemIdentifier;
                    ControlToken::Continue
                }
                Some("\0") => {
                    emitter.emit_error(Error::UnexpectedNullCharacter);
                    emitter.push_doctype_system_identifier("\u{fffd}");
                    ControlToken::Continue
                }
                Some(">") => {
                    emitter.emit_error(Error::AbruptDoctypeSystemIdentifier);
                    emitter.set_force_quirks();
                    machine_helper.state = State::Data;
                    emitter.emit_current_doctype();
                    ControlToken::Continue
                }
                Some(xs) => {
                    emitter.push_doctype_system_identifier(xs);
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInDoctype);
                    emitter.set_force_quirks();
                    emitter.emit_current_doctype();
                    ControlToken::Eof
                }
            }
        ),
        State::AfterDoctypeSystemIdentifier => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(whitespace_pat!()) => ControlToken::Continue,
                Some('>') => {
                    machine_helper.state = State::Data;
                    emitter.emit_current_doctype();
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInDoctype);
                    emitter.set_force_quirks();
                    emitter.emit_current_doctype();
                    ControlToken::Eof
                }
                c @ Some(_) => {
                    emitter.emit_error(Error::UnexpectedCharacterAfterDoctypeSystemIdentifier);
                    reconsume_in!(c, State::BogusDoctype)
                }
            }
        }),
        State::BogusDoctype => fast_read_char!(
            slf,
            emitter,
            machine_helper,
            match xs {
                Some(">") => {
                    machine_helper.state = State::Data;
                    emitter.emit_current_doctype();
                    ControlToken::Continue
                }
                Some("\0") => {
                    emitter.emit_error(Error::UnexpectedNullCharacter);
                    ControlToken::Continue
                }
                Some(_xs) => {
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_current_doctype();
                    ControlToken::Eof
                }
            }
        ),
        State::CdataSection => fast_read_char!(
            slf,
            emitter,
            machine_helper,
            match xs {
                Some("]") => {
                    machine_helper.state = State::CdataSectionBracket;
                    ControlToken::Continue
                }
                Some(xs) => {
                    emitter.emit_string(xs);
                    ControlToken::Continue
                }
                None => {
                    emitter.emit_error(Error::EofInCdata);
                    ControlToken::Eof
                }
            }
        ),
        State::CdataSectionBracket => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(']') => {
                    machine_helper.state = State::CdataSectionEnd;
                    ControlToken::Continue
                }
                c => {
                    emitter.emit_string("]");
                    reconsume_in!(c, State::CdataSection)
                }
            }
        }),
        State::CdataSectionEnd => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(']') => {
                    emitter.emit_string("]");
                    ControlToken::Continue
                }
                Some('>') => {
                    machine_helper.state = State::Data;
                    ControlToken::Continue
                }
                c => {
                    emitter.emit_string("]]");
                    reconsume_in!(c, State::CdataSection)
                }
            }
        }),
        State::CharacterReference => Ok({
            let emitter = &mut slf.emitter;
            machine_helper.temporary_buffer.clear();
            machine_helper.temporary_buffer.push('&');

            match slf.reader.read_char(emitter)? {
                Some(x) if x.is_ascii_alphanumeric() => {
                    reconsume_in!(Some(x), State::NamedCharacterReference)
                }
                Some('#') => {
                    machine_helper.temporary_buffer.push('#');
                    machine_helper.state = State::NumericCharacterReference;
                    ControlToken::Continue
                }
                c => {
                    machine_helper
                        .flush_code_points_consumed_as_character_reference(&mut slf.emitter);
                    reconsume_in!(c, machine_helper.pop_return_state())
                }
            }
        }),
        State::NamedCharacterReference => Ok({
            let emitter = &mut slf.emitter;
            let reader = &mut slf.reader;
            let c = reader.read_char(emitter)?;

            let char_ref = match c {
                Some(x) => try_read_character_reference(x, |x| reader.try_read_string(x, true))?
                    .map(|char_ref| (x, char_ref)),

                None => None,
            };

            if let Some((x, char_ref)) = char_ref {
                let char_ref_name_last_character = char_ref.name.chars().last();
                let next_character = reader.read_char(emitter)?;

                if !machine_helper.is_consumed_as_part_of_an_attribute()
                    || char_ref_name_last_character == Some(';')
                    || !matches!(next_character, Some(x) if x == '=' || x.is_ascii_alphanumeric())
                {
                    if char_ref_name_last_character != Some(';') {
                        emitter.emit_error(Error::MissingSemicolonAfterCharacterReference);
                    }

                    machine_helper.temporary_buffer.clear();
                    machine_helper
                        .temporary_buffer
                        .push_str(char_ref.characters);
                } else {
                    machine_helper.temporary_buffer.push(x);
                    machine_helper.temporary_buffer.push_str(char_ref.name);
                }

                machine_helper.flush_code_points_consumed_as_character_reference(emitter);
                reconsume_in!(next_character, machine_helper.pop_return_state())
            } else {
                machine_helper.flush_code_points_consumed_as_character_reference(emitter);
                reconsume_in!(c, State::AmbiguousAmpersand)
            }
        }),
        State::AmbiguousAmpersand => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(x) if x.is_ascii_alphanumeric() => {
                    if machine_helper.is_consumed_as_part_of_an_attribute() {
                        emitter.push_attribute_value(ctostr!(x));
                    } else {
                        emitter.emit_string(ctostr!(x));
                    }

                    ControlToken::Continue
                }
                c @ Some(';') => {
                    emitter.emit_error(Error::UnknownNamedCharacterReference);
                    reconsume_in!(c, machine_helper.pop_return_state())
                }
                c => {
                    reconsume_in!(c, machine_helper.pop_return_state())
                }
            }
        }),
        State::NumericCharacterReference => Ok({
            let emitter = &mut slf.emitter;
            machine_helper.character_reference_code = 0;

            match slf.reader.read_char(emitter)? {
                Some(x @ 'x' | x @ 'X') => {
                    machine_helper.temporary_buffer.push(x);
                    machine_helper.state = State::HexadecimalCharacterReferenceStart;
                    ControlToken::Continue
                }
                c => {
                    reconsume_in!(c, State::DecimalCharacterReferenceStart)
                }
            }
        }),
        State::HexadecimalCharacterReferenceStart => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                c @ Some('0'..='9' | 'A'..='F' | 'a'..='f') => {
                    reconsume_in!(c, State::HexadecimalCharacterReference)
                }
                c => {
                    emitter.emit_error(Error::AbsenceOfDigitsInNumericCharacterReference);
                    machine_helper
                        .flush_code_points_consumed_as_character_reference(&mut slf.emitter);
                    reconsume_in!(c, machine_helper.pop_return_state())
                }
            }
        }),
        State::DecimalCharacterReferenceStart => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(x @ ascii_digit_pat!()) => {
                    reconsume_in!(Some(x), State::DecimalCharacterReference)
                }
                c => {
                    emitter.emit_error(Error::AbsenceOfDigitsInNumericCharacterReference);
                    machine_helper
                        .flush_code_points_consumed_as_character_reference(&mut slf.emitter);
                    reconsume_in!(c, machine_helper.pop_return_state())
                }
            }
        }),
        State::HexadecimalCharacterReference => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(x @ ascii_digit_pat!()) => {
                    mutate_character_reference!(*16 + x - 0x0030);
                    ControlToken::Continue
                }
                Some(x @ 'A'..='F') => {
                    mutate_character_reference!(*16 + x - 0x0037);
                    ControlToken::Continue
                }
                Some(x @ 'a'..='f') => {
                    mutate_character_reference!(*16 + x - 0x0057);
                    ControlToken::Continue
                }
                Some(';') => {
                    machine_helper.state = State::NumericCharacterReferenceEnd;
                    ControlToken::Continue
                }
                c => {
                    emitter.emit_error(Error::MissingSemicolonAfterCharacterReference);
                    reconsume_in!(c, State::NumericCharacterReferenceEnd)
                }
            }
        }),
        State::DecimalCharacterReference => Ok({
            let emitter = &mut slf.emitter;
            match slf.reader.read_char(emitter)? {
                Some(x @ ascii_digit_pat!()) => {
                    mutate_character_reference!(*10 + x - 0x0030);
                    ControlToken::Continue
                }
                Some(';') => {
                    machine_helper.state = State::NumericCharacterReferenceEnd;
                    ControlToken::Continue
                }
                c => {
                    emitter.emit_error(Error::MissingSemicolonAfterCharacterReference);
                    reconsume_in!(c, State::NumericCharacterReferenceEnd)
                }
            }
        }),
        State::NumericCharacterReferenceEnd => Ok({
            let emitter = &mut slf.emitter;
            match machine_helper.character_reference_code {
                0x00 => {
                    emitter.emit_error(Error::NullCharacterReference);
                    machine_helper.character_reference_code = 0xfffd;
                }
                0x110000.. => {
                    emitter.emit_error(Error::CharacterReferenceOutsideUnicodeRange);
                    machine_helper.character_reference_code = 0xfffd;
                }
                surrogate_pat!() => {
                    emitter.emit_error(Error::SurrogateCharacterReference);
                    machine_helper.character_reference_code = 0xfffd;
                }
                // noncharacter
                noncharacter_pat!() => {
                    emitter.emit_error(Error::NoncharacterCharacterReference);
                }
                // 0x000d, or a control that is not whitespace
                x @ 0x000d | x @ control_pat!()
                    if !matches!(x, 0x0009 | 0x000a | 0x000c | 0x0020) =>
                {
                    emitter.emit_error(Error::ControlCharacterReference);
                    machine_helper.character_reference_code = match x {
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
                        _ => machine_helper.character_reference_code,
                    };
                }
                _ => (),
            }

            machine_helper.temporary_buffer.clear();
            machine_helper
                .temporary_buffer
                .push(std::char::from_u32(machine_helper.character_reference_code).unwrap());
            machine_helper.flush_code_points_consumed_as_character_reference(&mut slf.emitter);
            machine_helper.exit_state();
            ControlToken::Continue
        }),
    }
}
