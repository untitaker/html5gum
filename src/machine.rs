use crate::entities::try_read_character_reference;
use crate::utils::{
    ascii_digit_pat, control_pat, ctostr, noncharacter_pat, surrogate_pat, whitespace_pat,
    ControlToken, State,
};
use crate::{Emitter, Error, Reader, Tokenizer};

// Note: This is not implemented as a method on Tokenizer because there's fields on Tokenizer that
// should not be available in this method, such as Tokenizer.to_reconsume or the Reader instance
#[inline]
pub fn consume<R: Reader, E: Emitter<R>>(
    slf: &mut Tokenizer<R, E>,
) -> Result<ControlToken, R::Error> {
    macro_rules! mutate_character_reference {
        (* $mul:literal + $x:ident - $sub:literal) => {
            match slf
                .character_reference_code
                .checked_mul($mul)
                .and_then(|cr| cr.checked_add($x as u32 - $sub))
            {
                Some(cr) => slf.character_reference_code = cr,
                None => {
                    // provoke err
                    slf.character_reference_code = 0x110000;
                }
            };
        };
    }

    match slf.state {
        State::Data => match slf.read_char()? {
            Some('&') => {
                slf.return_state = Some(slf.state);
                slf.state = State::CharacterReference;
                Ok(ControlToken::Continue)
            }
            Some('<') => {
                slf.state = State::TagOpen;
                Ok(ControlToken::Continue)
            }
            Some('\0') => {
                slf.emit_error(Error::UnexpectedNullCharacter);
                slf.emitter.emit_string("\0");
                Ok(ControlToken::Continue)
            }
            Some(x) => {
                slf.emitter.emit_string(ctostr!(x));
                Ok(ControlToken::Continue)
            }
            None => Ok(ControlToken::Eof),
        },
        State::RcData => match slf.read_char()? {
            Some('&') => {
                slf.return_state = Some(State::RcData);
                slf.state = State::CharacterReference;
                Ok(ControlToken::Continue)
            }
            Some('<') => {
                slf.state = State::RcDataLessThanSign;
                Ok(ControlToken::Continue)
            }
            Some('\0') => {
                slf.emit_error(Error::UnexpectedNullCharacter);
                slf.emitter.emit_string("\u{fffd}");
                Ok(ControlToken::Continue)
            }
            Some(x) => {
                slf.emitter.emit_string(ctostr!(x));
                Ok(ControlToken::Continue)
            }
            None => Ok(ControlToken::Eof),
        },
        State::RawText => match slf.read_char()? {
            Some('<') => {
                slf.state = State::RawTextLessThanSign;
                Ok(ControlToken::Continue)
            }
            Some('\0') => {
                slf.emit_error(Error::UnexpectedNullCharacter);
                slf.emitter.emit_string("\u{fffd}");
                Ok(ControlToken::Continue)
            }
            Some(x) => {
                slf.emitter.emit_string(ctostr!(x));
                Ok(ControlToken::Continue)
            }
            None => Ok(ControlToken::Eof),
        },
        State::ScriptData => match slf.read_char()? {
            Some('<') => {
                slf.state = State::ScriptDataLessThanSign;
                Ok(ControlToken::Continue)
            }
            Some('\0') => {
                slf.emit_error(Error::UnexpectedNullCharacter);
                slf.emitter.emit_string("\u{fffd}");
                Ok(ControlToken::Continue)
            }
            Some(x) => {
                slf.emitter.emit_string(ctostr!(x));
                Ok(ControlToken::Continue)
            }
            None => Ok(ControlToken::Eof),
        },
        State::PlainText => match slf.read_char()? {
            Some('\0') => {
                slf.emit_error(Error::UnexpectedNullCharacter);
                slf.emitter.emit_string("\u{fffd}");
                Ok(ControlToken::Continue)
            }
            Some(x) => {
                slf.emitter.emit_string(ctostr!(x));
                Ok(ControlToken::Continue)
            }
            None => Ok(ControlToken::Eof),
        },
        State::TagOpen => match slf.read_char()? {
            Some('!') => {
                slf.state = State::MarkupDeclarationOpen;
                Ok(ControlToken::Continue)
            }
            Some('/') => {
                slf.state = State::EndTagOpen;
                Ok(ControlToken::Continue)
            }
            Some(x) if x.is_ascii_alphabetic() => {
                slf.emitter.init_start_tag(&slf.reader);
                slf.state = State::TagName;
                slf.unread_char(Some(x));
                Ok(ControlToken::Continue)
            }
            c @ Some('?') => {
                slf.emit_error(Error::UnexpectedQuestionMarkInsteadOfTagName);
                slf.emitter.init_comment(&slf.reader);
                slf.state = State::BogusComment;
                slf.unread_char(c);
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofBeforeTagName);
                slf.emitter.emit_string("<");
                Ok(ControlToken::Eof)
            }
            c @ Some(_) => {
                slf.emit_error(Error::InvalidFirstCharacterOfTagName);
                slf.state = State::Data;
                slf.emitter.emit_string("<");
                slf.unread_char(c);
                Ok(ControlToken::Continue)
            }
        },
        State::EndTagOpen => match slf.read_char()? {
            Some(x) if x.is_ascii_alphabetic() => {
                slf.emitter.init_end_tag(&slf.reader);
                slf.state = State::TagName;
                slf.unread_char(Some(x));
                Ok(ControlToken::Continue)
            }
            Some('>') => {
                slf.emit_error(Error::MissingEndTagName);
                slf.state = State::Data;
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofBeforeTagName);
                slf.emitter.emit_string("</");
                Ok(ControlToken::Eof)
            }
            Some(x) => {
                slf.emit_error(Error::InvalidFirstCharacterOfTagName);
                slf.emitter.init_comment(&slf.reader);
                slf.state = State::BogusComment;
                slf.unread_char(Some(x));
                Ok(ControlToken::Continue)
            }
        },
        State::TagName => match slf.read_char()? {
            Some(whitespace_pat!()) => {
                slf.state = State::BeforeAttributeName;
                Ok(ControlToken::Continue)
            }
            Some('/') => {
                slf.state = State::SelfClosingStartTag;
                Ok(ControlToken::Continue)
            }
            Some('>') => {
                slf.state = State::Data;
                slf.emitter.emit_current_tag();
                Ok(ControlToken::Continue)
            }
            Some('\0') => {
                slf.emit_error(Error::UnexpectedNullCharacter);
                slf.emitter.push_tag_name("\u{fffd}");
                Ok(ControlToken::Continue)
            }
            Some(x) => {
                slf.emitter.push_tag_name(ctostr!(x.to_ascii_lowercase()));
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInTag);
                Ok(ControlToken::Eof)
            }
        },
        State::RcDataLessThanSign => match slf.read_char()? {
            Some('/') => {
                slf.temporary_buffer.clear();
                slf.state = State::RcDataEndTagOpen;
                Ok(ControlToken::Continue)
            }
            c => {
                slf.emitter.emit_string("<");
                slf.state = State::RcData;
                slf.unread_char(c);
                Ok(ControlToken::Continue)
            }
        },
        State::RcDataEndTagOpen => match slf.read_char()? {
            Some(x) if x.is_ascii_alphabetic() => {
                slf.emitter.init_end_tag(&slf.reader);
                slf.state = State::RcDataEndTagName;
                slf.unread_char(Some(x));
                Ok(ControlToken::Continue)
            }
            c => {
                slf.emitter.emit_string("</");
                slf.state = State::RcData;
                slf.unread_char(c);
                Ok(ControlToken::Continue)
            }
        },
        State::RcDataEndTagName => match slf.read_char()? {
            Some(whitespace_pat!()) if slf.emitter.current_is_appropriate_end_tag_token() => {
                slf.state = State::BeforeAttributeName;
                Ok(ControlToken::Continue)
            }
            Some('/') if slf.emitter.current_is_appropriate_end_tag_token() => {
                slf.state = State::SelfClosingStartTag;
                Ok(ControlToken::Continue)
            }
            Some('>') if slf.emitter.current_is_appropriate_end_tag_token() => {
                slf.state = State::Data;
                slf.emitter.emit_current_tag();
                Ok(ControlToken::Continue)
            }
            Some(x) if x.is_ascii_alphabetic() => {
                slf.emitter.push_tag_name(ctostr!(x.to_ascii_lowercase()));
                slf.temporary_buffer.push(x);
                Ok(ControlToken::Continue)
            }
            c => {
                slf.emitter.emit_string("</");
                slf.flush_buffer_characters();

                slf.state = State::RcData;
                slf.unread_char(c);
                Ok(ControlToken::Continue)
            }
        },
        State::RawTextLessThanSign => match slf.read_char()? {
            Some('/') => {
                slf.temporary_buffer.clear();
                slf.state = State::RawTextEndTagOpen;
                Ok(ControlToken::Continue)
            }
            c => {
                slf.emitter.emit_string("<");
                slf.state = State::RawText;
                slf.unread_char(c);
                Ok(ControlToken::Continue)
            }
        },
        State::RawTextEndTagOpen => match slf.read_char()? {
            Some(x) if x.is_ascii_alphabetic() => {
                slf.emitter.init_end_tag(&slf.reader);
                slf.state = State::RawTextEndTagName;
                slf.unread_char(Some(x));
                Ok(ControlToken::Continue)
            }
            c => {
                slf.emitter.emit_string("</");
                slf.state = State::RawText;
                slf.unread_char(c);
                Ok(ControlToken::Continue)
            }
        },
        State::RawTextEndTagName => match slf.read_char()? {
            Some(whitespace_pat!()) if slf.emitter.current_is_appropriate_end_tag_token() => {
                slf.state = State::BeforeAttributeName;
                Ok(ControlToken::Continue)
            }
            Some('/') if slf.emitter.current_is_appropriate_end_tag_token() => {
                slf.state = State::SelfClosingStartTag;
                Ok(ControlToken::Continue)
            }
            Some('>') if slf.emitter.current_is_appropriate_end_tag_token() => {
                slf.state = State::Data;
                slf.emitter.emit_current_tag();
                Ok(ControlToken::Continue)
            }
            Some(x) if x.is_ascii_alphabetic() => {
                slf.emitter.push_tag_name(ctostr!(x.to_ascii_lowercase()));
                slf.temporary_buffer.push(x);
                Ok(ControlToken::Continue)
            }
            c => {
                slf.emitter.emit_string("</");
                slf.flush_buffer_characters();

                slf.state = State::RawText;
                slf.unread_char(c);
                Ok(ControlToken::Continue)
            }
        },
        State::ScriptDataLessThanSign => match slf.read_char()? {
            Some('/') => {
                slf.temporary_buffer.clear();
                slf.state = State::ScriptDataEndTagOpen;
                Ok(ControlToken::Continue)
            }
            Some('!') => {
                slf.state = State::ScriptDataEscapeStart;
                slf.emitter.emit_string("<!");
                Ok(ControlToken::Continue)
            }
            c => {
                slf.emitter.emit_string("<");
                slf.state = State::Data;
                slf.unread_char(c);
                Ok(ControlToken::Continue)
            }
        },
        State::ScriptDataEndTagOpen => match slf.read_char()? {
            Some(x) if x.is_ascii_alphabetic() => {
                slf.emitter.init_end_tag(&slf.reader);
                slf.state = State::ScriptDataEndTagName;
                slf.unread_char(Some(x));
                Ok(ControlToken::Continue)
            }
            c => {
                slf.emitter.emit_string("</");
                slf.state = State::ScriptData;
                slf.unread_char(c);
                Ok(ControlToken::Continue)
            }
        },
        State::ScriptDataEndTagName => match slf.read_char()? {
            Some(whitespace_pat!()) if slf.emitter.current_is_appropriate_end_tag_token() => {
                slf.state = State::BeforeAttributeName;
                Ok(ControlToken::Continue)
            }
            Some('/') if slf.emitter.current_is_appropriate_end_tag_token() => {
                slf.state = State::SelfClosingStartTag;
                Ok(ControlToken::Continue)
            }
            Some('>') if slf.emitter.current_is_appropriate_end_tag_token() => {
                slf.state = State::Data;
                slf.emitter.emit_current_tag();
                Ok(ControlToken::Continue)
            }
            Some(x) if x.is_ascii_alphabetic() => {
                slf.emitter.push_tag_name(ctostr!(x.to_ascii_lowercase()));
                slf.temporary_buffer.push(x.to_ascii_lowercase());
                Ok(ControlToken::Continue)
            }
            c => {
                slf.emitter.emit_string("</");
                slf.flush_buffer_characters();
                slf.state = State::Data;
                slf.unread_char(c);
                Ok(ControlToken::Continue)
            }
        },
        State::ScriptDataEscapeStart => match slf.read_char()? {
            Some('-') => {
                slf.state = State::ScriptDataEscapeStartDash;
                slf.emitter.emit_string("-");
                Ok(ControlToken::Continue)
            }
            c => {
                slf.state = State::ScriptData;
                slf.unread_char(c);
                Ok(ControlToken::Continue)
            }
        },
        State::ScriptDataEscapeStartDash => match slf.read_char()? {
            Some('-') => {
                slf.state = State::ScriptDataEscapedDashDash;
                slf.emitter.emit_string("-");
                Ok(ControlToken::Continue)
            }
            c => {
                slf.state = State::ScriptData;
                slf.unread_char(c);
                Ok(ControlToken::Continue)
            }
        },
        State::ScriptDataEscaped => match slf.read_char()? {
            Some('-') => {
                slf.state = State::ScriptDataEscapedDash;
                slf.emitter.emit_string("-");
                Ok(ControlToken::Continue)
            }
            Some('<') => {
                slf.state = State::ScriptDataEscapedLessThanSign;
                Ok(ControlToken::Continue)
            }
            Some('\0') => {
                slf.emit_error(Error::UnexpectedNullCharacter);
                slf.emitter.emit_string("\u{fffd}");
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInScriptHtmlCommentLikeText);
                Ok(ControlToken::Eof)
            }
            Some(x) => {
                slf.emitter.emit_string(ctostr!(x));
                Ok(ControlToken::Continue)
            }
        },
        State::ScriptDataEscapedDash => match slf.read_char()? {
            Some('-') => {
                slf.state = State::ScriptDataEscapedDashDash;
                slf.emitter.emit_string("-");
                Ok(ControlToken::Continue)
            }
            Some('<') => {
                slf.state = State::ScriptDataEscapedLessThanSign;
                Ok(ControlToken::Continue)
            }
            Some('\0') => {
                slf.emit_error(Error::UnexpectedNullCharacter);
                slf.state = State::ScriptDataEscaped;
                slf.emitter.emit_string("\u{fffd}");
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInScriptHtmlCommentLikeText);
                Ok(ControlToken::Eof)
            }
            Some(x) => {
                slf.state = State::ScriptDataEscaped;
                slf.emitter.emit_string(ctostr!(x));
                Ok(ControlToken::Continue)
            }
        },
        State::ScriptDataEscapedDashDash => match slf.read_char()? {
            Some('-') => {
                slf.emitter.emit_string("-");
                Ok(ControlToken::Continue)
            }
            Some('<') => {
                slf.state = State::ScriptDataEscapedLessThanSign;
                Ok(ControlToken::Continue)
            }
            Some('>') => {
                slf.state = State::ScriptData;
                slf.emitter.emit_string(">");
                Ok(ControlToken::Continue)
            }
            Some('\0') => {
                slf.emit_error(Error::UnexpectedNullCharacter);
                slf.state = State::ScriptDataEscaped;
                slf.emitter.emit_string("\u{fffd}");
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInScriptHtmlCommentLikeText);
                Ok(ControlToken::Eof)
            }
            Some(x) => {
                slf.state = State::ScriptDataEscaped;
                slf.emitter.emit_string(ctostr!(x));
                Ok(ControlToken::Continue)
            }
        },
        State::ScriptDataEscapedLessThanSign => match slf.read_char()? {
            Some('/') => {
                slf.temporary_buffer.clear();
                slf.state = State::ScriptDataEscapedEndTagOpen;
                Ok(ControlToken::Continue)
            }
            Some(x) if x.is_ascii_alphabetic() => {
                slf.temporary_buffer.clear();
                slf.emitter.emit_string("<");
                slf.state = State::ScriptDataDoubleEscapeStart;
                slf.unread_char(Some(x));
                Ok(ControlToken::Continue)
            }
            c => {
                slf.emitter.emit_string("<");
                slf.state = State::ScriptDataEscaped;
                slf.unread_char(c);
                Ok(ControlToken::Continue)
            }
        },
        State::ScriptDataEscapedEndTagOpen => match slf.read_char()? {
            Some(x) if x.is_ascii_alphabetic() => {
                slf.emitter.init_end_tag(&slf.reader);
                slf.state = State::ScriptDataEscapedEndTagName;
                slf.unread_char(Some(x));
                Ok(ControlToken::Continue)
            }
            c => {
                slf.emitter.emit_string("</");
                slf.unread_char(c);
                slf.state = State::ScriptDataEscaped;
                Ok(ControlToken::Continue)
            }
        },
        State::ScriptDataEscapedEndTagName => match slf.read_char()? {
            Some(whitespace_pat!()) if slf.emitter.current_is_appropriate_end_tag_token() => {
                slf.state = State::BeforeAttributeName;
                Ok(ControlToken::Continue)
            }
            Some('/') if slf.emitter.current_is_appropriate_end_tag_token() => {
                slf.state = State::SelfClosingStartTag;
                Ok(ControlToken::Continue)
            }
            Some('>') if slf.emitter.current_is_appropriate_end_tag_token() => {
                slf.state = State::Data;
                slf.emitter.emit_current_tag();
                Ok(ControlToken::Continue)
            }
            Some(x) if x.is_ascii_alphabetic() => {
                slf.emitter.push_tag_name(ctostr!(x.to_ascii_lowercase()));
                slf.temporary_buffer.push(x);
                Ok(ControlToken::Continue)
            }
            c => {
                slf.emitter.emit_string("</");
                slf.flush_buffer_characters();
                slf.state = State::ScriptDataEscaped;
                slf.unread_char(c);
                Ok(ControlToken::Continue)
            }
        },
        State::ScriptDataDoubleEscapeStart => match slf.read_char()? {
            Some(x @ whitespace_pat!() | x @ '/' | x @ '>') => {
                if slf.temporary_buffer == "script" {
                    slf.state = State::ScriptDataDoubleEscaped;
                } else {
                    slf.state = State::ScriptDataEscaped;
                }
                slf.emitter.emit_string(ctostr!(x));
                Ok(ControlToken::Continue)
            }
            Some(x) if x.is_ascii_alphabetic() => {
                slf.temporary_buffer.push(x.to_ascii_lowercase());
                slf.emitter.emit_string(ctostr!(x));
                Ok(ControlToken::Continue)
            }
            c => {
                slf.state = State::ScriptDataEscaped;
                slf.unread_char(c);
                Ok(ControlToken::Continue)
            }
        },
        State::ScriptDataDoubleEscaped => match slf.read_char()? {
            Some('-') => {
                slf.state = State::ScriptDataDoubleEscapedDash;
                slf.emitter.emit_string("-");
                Ok(ControlToken::Continue)
            }
            Some('<') => {
                slf.state = State::ScriptDataDoubleEscapedLessThanSign;
                slf.emitter.emit_string("<");
                Ok(ControlToken::Continue)
            }
            Some('\0') => {
                slf.emit_error(Error::UnexpectedNullCharacter);
                slf.emitter.emit_string("\u{fffd}");
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInScriptHtmlCommentLikeText);
                Ok(ControlToken::Eof)
            }
            Some(x) => {
                slf.emitter.emit_string(ctostr!(x));
                Ok(ControlToken::Continue)
            }
        },
        State::ScriptDataDoubleEscapedDash => match slf.read_char()? {
            Some('-') => {
                slf.state = State::ScriptDataDoubleEscapedDashDash;
                slf.emitter.emit_string("-");
                Ok(ControlToken::Continue)
            }
            Some('<') => {
                slf.state = State::ScriptDataDoubleEscapedLessThanSign;
                slf.emitter.emit_string("<");
                Ok(ControlToken::Continue)
            }
            Some('\0') => {
                slf.emit_error(Error::UnexpectedNullCharacter);
                slf.state = State::ScriptDataDoubleEscaped;
                slf.emitter.emit_string("\u{fffd}");
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInScriptHtmlCommentLikeText);
                Ok(ControlToken::Eof)
            }
            Some(x) => {
                slf.state = State::ScriptDataDoubleEscaped;
                slf.emitter.emit_string(ctostr!(x));
                Ok(ControlToken::Continue)
            }
        },
        State::ScriptDataDoubleEscapedDashDash => match slf.read_char()? {
            Some('-') => {
                slf.emitter.emit_string("-");
                Ok(ControlToken::Continue)
            }
            Some('<') => {
                slf.emitter.emit_string("<");
                slf.state = State::ScriptDataDoubleEscapedLessThanSign;
                Ok(ControlToken::Continue)
            }
            Some('>') => {
                slf.emitter.emit_string(">");
                slf.state = State::ScriptData;
                Ok(ControlToken::Continue)
            }
            Some('\0') => {
                slf.emit_error(Error::UnexpectedNullCharacter);
                slf.state = State::ScriptDataDoubleEscaped;
                slf.emitter.emit_string("\u{fffd}");
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInScriptHtmlCommentLikeText);
                Ok(ControlToken::Eof)
            }
            Some(x) => {
                slf.state = State::ScriptDataDoubleEscaped;
                slf.emitter.emit_string(ctostr!(x));
                Ok(ControlToken::Continue)
            }
        },
        State::ScriptDataDoubleEscapedLessThanSign => match slf.read_char()? {
            Some('/') => {
                slf.temporary_buffer.clear();
                slf.state = State::ScriptDataDoubleEscapeEnd;
                slf.emitter.emit_string("/");
                Ok(ControlToken::Continue)
            }
            c => {
                slf.state = State::ScriptDataDoubleEscaped;
                slf.unread_char(c);
                Ok(ControlToken::Continue)
            }
        },
        State::ScriptDataDoubleEscapeEnd => match slf.read_char()? {
            Some(x @ whitespace_pat!() | x @ '/' | x @ '>') => {
                if slf.temporary_buffer == "script" {
                    slf.state = State::ScriptDataEscaped;
                } else {
                    slf.state = State::ScriptDataDoubleEscaped;
                }

                slf.emitter.emit_string(ctostr!(x));
                Ok(ControlToken::Continue)
            }
            Some(x) if x.is_ascii_alphabetic() => {
                slf.temporary_buffer.push(x.to_ascii_lowercase());
                slf.emitter.emit_string(ctostr!(x));
                Ok(ControlToken::Continue)
            }
            c => {
                slf.state = State::ScriptDataDoubleEscaped;
                slf.unread_char(c);
                Ok(ControlToken::Continue)
            }
        },
        State::BeforeAttributeName => match slf.read_char()? {
            Some(whitespace_pat!()) => Ok(ControlToken::Continue),
            c @ Some('/' | '>') | c @ None => {
                slf.state = State::AfterAttributeName;
                slf.unread_char(c);
                Ok(ControlToken::Continue)
            }
            Some('=') => {
                slf.emit_error(Error::UnexpectedEqualsSignBeforeAttributeName);
                slf.emitter.init_attribute_name(&slf.reader);
                slf.emitter.push_attribute_name("=");
                slf.state = State::AttributeName;
                Ok(ControlToken::Continue)
            }
            Some(x) => {
                slf.emitter.init_attribute_name(&slf.reader);
                slf.state = State::AttributeName;
                slf.unread_char(Some(x));
                Ok(ControlToken::Continue)
            }
        },
        State::AttributeName => match slf.read_char()? {
            c @ Some(whitespace_pat!() | '/' | '>') | c @ None => {
                slf.state = State::AfterAttributeName;
                slf.unread_char(c);
                Ok(ControlToken::Continue)
            }
            Some('=') => {
                slf.state = State::BeforeAttributeValue;
                Ok(ControlToken::Continue)
            }
            Some('\0') => {
                slf.emit_error(Error::UnexpectedNullCharacter);
                slf.emitter.push_attribute_name("\u{fffd}");
                Ok(ControlToken::Continue)
            }
            Some(x @ '"' | x @ '\'' | x @ '<') => {
                slf.emit_error(Error::UnexpectedCharacterInAttributeName);
                slf.emitter
                    .push_attribute_name(ctostr!(x.to_ascii_lowercase()));
                Ok(ControlToken::Continue)
            }
            Some(x) => {
                slf.emitter
                    .push_attribute_name(ctostr!(x.to_ascii_lowercase()));
                Ok(ControlToken::Continue)
            }
        },
        State::AfterAttributeName => match slf.read_char()? {
            Some(whitespace_pat!()) => Ok(ControlToken::Continue),
            Some('/') => {
                slf.state = State::SelfClosingStartTag;
                Ok(ControlToken::Continue)
            }
            Some('=') => {
                slf.state = State::BeforeAttributeValue;
                Ok(ControlToken::Continue)
            }
            Some('>') => {
                slf.state = State::Data;
                slf.emitter.emit_current_tag();
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInTag);
                Ok(ControlToken::Eof)
            }
            Some(x) => {
                slf.emitter.init_attribute_name(&slf.reader);
                slf.state = State::AttributeName;
                slf.unread_char(Some(x));
                Ok(ControlToken::Continue)
            }
        },
        State::BeforeAttributeValue => match slf.read_char()? {
            Some(whitespace_pat!()) => Ok(ControlToken::Continue),
            Some('"') => {
                slf.emitter.init_attribute_value(&slf.reader, true);
                slf.state = State::AttributeValueDoubleQuoted;
                Ok(ControlToken::Continue)
            }
            Some('\'') => {
                slf.emitter.init_attribute_value(&slf.reader, true);
                slf.state = State::AttributeValueSingleQuoted;
                Ok(ControlToken::Continue)
            }
            Some('>') => {
                slf.emit_error(Error::MissingAttributeValue);
                slf.state = State::Data;
                slf.emitter.emit_current_tag();
                Ok(ControlToken::Continue)
            }
            c => {
                slf.emitter.init_attribute_value(&slf.reader, false);
                slf.state = State::AttributeValueUnquoted;
                slf.unread_char(c);
                Ok(ControlToken::Continue)
            }
        },
        State::AttributeValueDoubleQuoted => match slf.read_char()? {
            Some('"') => {
                slf.state = State::AfterAttributeValueQuoted;
                Ok(ControlToken::Continue)
            }
            Some('&') => {
                slf.return_state = Some(State::AttributeValueDoubleQuoted);
                slf.state = State::CharacterReference;
                Ok(ControlToken::Continue)
            }
            Some('\0') => {
                slf.emit_error(Error::UnexpectedNullCharacter);
                slf.emitter.push_attribute_value("\u{fffd}");
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInTag);
                Ok(ControlToken::Eof)
            }
            Some(x) => {
                slf.emitter.push_attribute_value(ctostr!(x));
                Ok(ControlToken::Continue)
            }
        },
        State::AttributeValueSingleQuoted => match slf.read_char()? {
            Some('\'') => {
                slf.state = State::AfterAttributeValueQuoted;
                Ok(ControlToken::Continue)
            }
            Some('&') => {
                slf.return_state = Some(State::AttributeValueSingleQuoted);
                slf.state = State::CharacterReference;
                Ok(ControlToken::Continue)
            }
            Some('\0') => {
                slf.emit_error(Error::UnexpectedNullCharacter);
                slf.emitter.push_attribute_value("\u{fffd}");
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInTag);
                Ok(ControlToken::Eof)
            }
            Some(x) => {
                slf.emitter.push_attribute_value(ctostr!(x));
                Ok(ControlToken::Continue)
            }
        },
        State::AttributeValueUnquoted => match slf.read_char()? {
            Some(whitespace_pat!()) => {
                slf.state = State::BeforeAttributeName;
                Ok(ControlToken::Continue)
            }
            Some('&') => {
                slf.return_state = Some(State::AttributeValueUnquoted);
                slf.state = State::CharacterReference;
                Ok(ControlToken::Continue)
            }
            Some('>') => {
                slf.state = State::Data;
                slf.emitter.emit_current_tag();
                Ok(ControlToken::Continue)
            }
            Some('\0') => {
                slf.emit_error(Error::UnexpectedNullCharacter);
                slf.emitter.push_attribute_value("\u{fffd}");
                Ok(ControlToken::Continue)
            }
            Some(x @ '"' | x @ '\'' | x @ '<' | x @ '=' | x @ '\u{60}') => {
                slf.emit_error(Error::UnexpectedCharacterInUnquotedAttributeValue);
                slf.emitter.push_attribute_value(ctostr!(x));
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInTag);
                Ok(ControlToken::Eof)
            }
            Some(x) => {
                slf.emitter.push_attribute_value(ctostr!(x));
                Ok(ControlToken::Continue)
            }
        },
        State::AfterAttributeValueQuoted => match slf.read_char()? {
            Some(whitespace_pat!()) => {
                slf.state = State::BeforeAttributeName;
                Ok(ControlToken::Continue)
            }
            Some('/') => {
                slf.state = State::SelfClosingStartTag;
                Ok(ControlToken::Continue)
            }
            Some('>') => {
                slf.state = State::Data;
                slf.emitter.emit_current_tag();
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInTag);
                Ok(ControlToken::Eof)
            }
            Some(x) => {
                slf.emit_error(Error::MissingWhitespaceBetweenAttributes);
                slf.state = State::BeforeAttributeName;
                slf.unread_char(Some(x));
                Ok(ControlToken::Continue)
            }
        },
        State::SelfClosingStartTag => match slf.read_char()? {
            Some('>') => {
                slf.emitter.set_self_closing(&slf.reader);
                slf.state = State::Data;
                slf.emitter.emit_current_tag();
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInTag);
                Ok(ControlToken::Eof)
            }
            Some(x) => {
                slf.emit_error(Error::UnexpectedSolidusInTag);
                slf.state = State::BeforeAttributeName;
                slf.unread_char(Some(x));
                Ok(ControlToken::Continue)
            }
        },
        State::BogusComment => match slf.read_char()? {
            Some('>') => {
                slf.state = State::Data;
                slf.emitter.emit_current_comment();
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emitter.emit_current_comment();
                Ok(ControlToken::Eof)
            }
            Some('\0') => {
                slf.emit_error(Error::UnexpectedNullCharacter);
                slf.emitter.push_comment("\u{fffd}");
                Ok(ControlToken::Continue)
            }
            Some(x) => {
                slf.emitter.push_comment(ctostr!(x));
                Ok(ControlToken::Continue)
            }
        },
        State::MarkupDeclarationOpen => match slf.read_char()? {
            Some('-') if slf.try_read_string("-", true)? => {
                slf.emitter.init_comment(&slf.reader);
                slf.state = State::CommentStart;
                Ok(ControlToken::Continue)
            }
            Some('d' | 'D') if slf.try_read_string("octype", false)? => {
                slf.state = State::Doctype;
                Ok(ControlToken::Continue)
            }
            Some('[') if slf.try_read_string("CDATA[", true)? => {
                // missing: check for adjusted current element: we don't have an element stack
                // at all
                //
                // missing: cdata transition
                //
                // let's hope that bogus comment can just sort of skip over cdata
                slf.emit_error(Error::CdataInHtmlContent);

                slf.emitter.init_comment(&slf.reader);
                slf.emitter.push_comment("[CDATA[");
                slf.state = State::BogusComment;
                Ok(ControlToken::Continue)
            }
            c => {
                slf.emit_error(Error::IncorrectlyOpenedComment);
                slf.emitter.init_comment(&slf.reader);
                slf.state = State::BogusComment;
                slf.unread_char(c);
                Ok(ControlToken::Continue)
            }
        },
        State::CommentStart => match slf.read_char()? {
            Some('-') => {
                slf.state = State::CommentStartDash;
                Ok(ControlToken::Continue)
            }
            Some('>') => {
                slf.emit_error(Error::AbruptClosingOfEmptyComment);
                slf.state = State::Data;
                slf.emitter.emit_current_comment();
                Ok(ControlToken::Continue)
            }
            c => {
                slf.unread_char(c);
                slf.state = State::Comment;
                Ok(ControlToken::Continue)
            }
        },
        State::CommentStartDash => match slf.read_char()? {
            Some('-') => {
                slf.state = State::CommentEnd;
                Ok(ControlToken::Continue)
            }
            Some('>') => {
                slf.emit_error(Error::AbruptClosingOfEmptyComment);
                slf.state = State::Data;
                slf.emitter.emit_current_comment();
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInComment);
                slf.emitter.emit_current_comment();
                Ok(ControlToken::Eof)
            }
            c @ Some(_) => {
                slf.emitter.push_comment("-");
                slf.unread_char(c);
                slf.state = State::Comment;
                Ok(ControlToken::Continue)
            }
        },
        State::Comment => match slf.read_char()? {
            Some('<') => {
                slf.emitter.push_comment("<");
                slf.state = State::CommentLessThanSign;
                Ok(ControlToken::Continue)
            }
            Some('-') => {
                slf.state = State::CommentEndDash;
                Ok(ControlToken::Continue)
            }
            Some('\0') => {
                slf.emit_error(Error::UnexpectedNullCharacter);
                slf.emitter.push_comment("\u{fffd}");
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInComment);
                slf.emitter.emit_current_comment();
                Ok(ControlToken::Eof)
            }
            Some(x) => {
                slf.emitter.push_comment(ctostr!(x));
                Ok(ControlToken::Continue)
            }
        },
        State::CommentLessThanSign => match slf.read_char()? {
            Some('!') => {
                slf.emitter.push_comment("!");
                slf.state = State::CommentLessThanSignBang;
                Ok(ControlToken::Continue)
            }
            Some('<') => {
                slf.emitter.push_comment("<");
                Ok(ControlToken::Continue)
            }
            c => {
                slf.unread_char(c);
                slf.state = State::Comment;
                Ok(ControlToken::Continue)
            }
        },
        State::CommentLessThanSignBang => match slf.read_char()? {
            Some('-') => {
                slf.state = State::CommentLessThanSignBangDash;
                Ok(ControlToken::Continue)
            }
            c => {
                slf.unread_char(c);
                slf.state = State::Comment;
                Ok(ControlToken::Continue)
            }
        },
        State::CommentLessThanSignBangDash => match slf.read_char()? {
            Some('-') => {
                slf.state = State::CommentLessThanSignBangDashDash;
                Ok(ControlToken::Continue)
            }
            c => {
                slf.unread_char(c);
                slf.state = State::CommentEndDash;
                Ok(ControlToken::Continue)
            }
        },
        State::CommentLessThanSignBangDashDash => match slf.read_char()? {
            c @ Some('>') | c @ None => {
                slf.unread_char(c);
                slf.state = State::CommentEnd;
                Ok(ControlToken::Continue)
            }
            c => {
                slf.emit_error(Error::NestedComment);
                slf.unread_char(c);
                slf.state = State::CommentEnd;
                Ok(ControlToken::Continue)
            }
        },
        State::CommentEndDash => match slf.read_char()? {
            Some('-') => {
                slf.state = State::CommentEnd;
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInComment);
                slf.emitter.emit_current_comment();
                Ok(ControlToken::Eof)
            }
            c => {
                slf.emitter.push_comment("-");
                slf.unread_char(c);
                slf.state = State::Comment;
                Ok(ControlToken::Continue)
            }
        },
        State::CommentEnd => match slf.read_char()? {
            Some('>') => {
                slf.state = State::Data;
                slf.emitter.emit_current_comment();
                Ok(ControlToken::Continue)
            }
            Some('!') => {
                slf.state = State::CommentEndBang;
                Ok(ControlToken::Continue)
            }
            Some('-') => {
                slf.emitter.push_comment("-");
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInComment);
                slf.emitter.emit_current_comment();
                Ok(ControlToken::Eof)
            }
            c @ Some(_) => {
                slf.emitter.push_comment("-");
                slf.emitter.push_comment("-");
                slf.unread_char(c);
                slf.state = State::Comment;
                Ok(ControlToken::Continue)
            }
        },
        State::CommentEndBang => match slf.read_char()? {
            Some('-') => {
                slf.emitter.push_comment("-");
                slf.emitter.push_comment("-");
                slf.emitter.push_comment("!");
                slf.state = State::CommentEndDash;
                Ok(ControlToken::Continue)
            }
            Some('>') => {
                slf.emit_error(Error::IncorrectlyClosedComment);
                slf.state = State::Data;
                slf.emitter.emit_current_comment();
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInComment);
                slf.emitter.emit_current_comment();
                Ok(ControlToken::Eof)
            }
            c @ Some(_) => {
                slf.emitter.push_comment("-");
                slf.emitter.push_comment("-");
                slf.emitter.push_comment("!");
                slf.state = State::Comment;
                slf.unread_char(c);
                Ok(ControlToken::Continue)
            }
        },
        State::Doctype => match slf.read_char()? {
            Some(whitespace_pat!()) => {
                slf.state = State::BeforeDoctypeName;
                Ok(ControlToken::Continue)
            }
            c @ Some('>') => {
                slf.unread_char(c);
                slf.state = State::BeforeDoctypeName;
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInDoctype);
                slf.emitter.init_doctype(&slf.reader);
                slf.emitter.set_force_quirks();
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Eof)
            }
            c @ Some(_) => {
                slf.emit_error(Error::MissingWhitespaceBeforeDoctypeName);
                slf.unread_char(c);
                slf.state = State::BeforeDoctypeName;
                Ok(ControlToken::Continue)
            }
        },
        State::BeforeDoctypeName => match slf.read_char()? {
            Some(whitespace_pat!()) => Ok(ControlToken::Continue),
            Some('\0') => {
                slf.emit_error(Error::UnexpectedNullCharacter);
                slf.emitter.init_doctype(&slf.reader);
                slf.emitter.push_doctype_name("\u{fffd}");
                slf.state = State::DoctypeName;
                Ok(ControlToken::Continue)
            }
            Some('>') => {
                slf.emit_error(Error::MissingDoctypeName);
                slf.emitter.init_doctype(&slf.reader);
                slf.emitter.set_force_quirks();
                slf.state = State::Data;
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInDoctype);
                slf.emitter.init_doctype(&slf.reader);
                slf.emitter.set_force_quirks();
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Eof)
            }
            Some(x) => {
                slf.emitter.init_doctype(&slf.reader);
                slf.emitter
                    .push_doctype_name(ctostr!(x.to_ascii_lowercase()));
                slf.state = State::DoctypeName;
                Ok(ControlToken::Continue)
            }
        },
        State::DoctypeName => match slf.read_char()? {
            Some(whitespace_pat!()) => {
                slf.state = State::AfterDoctypeName;
                Ok(ControlToken::Continue)
            }
            Some('>') => {
                slf.state = State::Data;
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Continue)
            }
            Some('\0') => {
                slf.emit_error(Error::UnexpectedNullCharacter);
                slf.emitter.push_doctype_name("\u{fffd}");
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInDoctype);
                slf.emitter.set_force_quirks();
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Eof)
            }
            Some(x) => {
                slf.emitter
                    .push_doctype_name(ctostr!(x.to_ascii_lowercase()));
                Ok(ControlToken::Continue)
            }
        },
        State::AfterDoctypeName => match slf.read_char()? {
            Some(whitespace_pat!()) => Ok(ControlToken::Continue),
            Some('>') => {
                slf.state = State::Data;
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInDoctype);
                slf.emitter.set_force_quirks();
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Eof)
            }
            Some('p' | 'P') if slf.try_read_string("ublic", false)? => {
                slf.state = State::AfterDoctypePublicKeyword;
                Ok(ControlToken::Continue)
            }
            Some('s' | 'S') if slf.try_read_string("ystem", false)? => {
                slf.state = State::AfterDoctypeSystemKeyword;
                Ok(ControlToken::Continue)
            }
            c @ Some(_) => {
                slf.emit_error(Error::InvalidCharacterSequenceAfterDoctypeName);
                slf.emitter.set_force_quirks();
                slf.unread_char(c);
                slf.state = State::BogusDoctype;
                Ok(ControlToken::Continue)
            }
        },
        State::AfterDoctypePublicKeyword => match slf.read_char()? {
            Some(whitespace_pat!()) => {
                slf.state = State::BeforeDoctypePublicIdentifier;
                Ok(ControlToken::Continue)
            }
            Some('"') => {
                slf.emit_error(Error::MissingWhitespaceAfterDoctypePublicKeyword);
                slf.emitter.set_doctype_public_identifier("");
                slf.state = State::DoctypePublicIdentifierDoubleQuoted;
                Ok(ControlToken::Continue)
            }
            Some('\'') => {
                slf.emit_error(Error::MissingWhitespaceAfterDoctypePublicKeyword);
                slf.emitter.set_doctype_public_identifier("");
                slf.state = State::DoctypePublicIdentifierSingleQuoted;
                Ok(ControlToken::Continue)
            }
            Some('>') => {
                slf.emit_error(Error::MissingDoctypePublicIdentifier);
                slf.emitter.set_force_quirks();
                slf.state = State::Data;
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInDoctype);
                slf.emitter.set_force_quirks();
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Eof)
            }
            c @ Some(_) => {
                slf.emit_error(Error::MissingQuoteBeforeDoctypePublicIdentifier);
                slf.emitter.set_force_quirks();
                slf.unread_char(c);
                slf.state = State::BogusDoctype;
                Ok(ControlToken::Continue)
            }
        },
        State::BeforeDoctypePublicIdentifier => match slf.read_char()? {
            Some(whitespace_pat!()) => Ok(ControlToken::Continue),
            Some('"') => {
                slf.emitter.set_doctype_public_identifier("");
                slf.state = State::DoctypePublicIdentifierDoubleQuoted;
                Ok(ControlToken::Continue)
            }
            Some('\'') => {
                slf.emitter.set_doctype_public_identifier("");
                slf.state = State::DoctypePublicIdentifierSingleQuoted;
                Ok(ControlToken::Continue)
            }
            Some('>') => {
                slf.emit_error(Error::MissingDoctypePublicIdentifier);
                slf.emitter.set_force_quirks();
                slf.state = State::Data;
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInDoctype);
                slf.emitter.set_force_quirks();
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Eof)
            }
            c @ Some(_) => {
                slf.emit_error(Error::MissingQuoteBeforeDoctypePublicIdentifier);
                slf.emitter.set_force_quirks();
                slf.unread_char(c);
                slf.state = State::BogusDoctype;
                Ok(ControlToken::Continue)
            }
        },
        State::DoctypePublicIdentifierDoubleQuoted => match slf.read_char()? {
            Some('"') => {
                slf.state = State::AfterDoctypePublicIdentifier;
                Ok(ControlToken::Continue)
            }
            Some('\0') => {
                slf.emit_error(Error::UnexpectedNullCharacter);
                slf.emitter.push_doctype_public_identifier("\u{fffd}");
                Ok(ControlToken::Continue)
            }
            Some('>') => {
                slf.emit_error(Error::AbruptDoctypePublicIdentifier);
                slf.emitter.set_force_quirks();
                slf.state = State::Data;
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInDoctype);
                slf.emitter.set_force_quirks();
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Eof)
            }
            Some(x) => {
                slf.emitter.push_doctype_public_identifier(ctostr!(x));
                Ok(ControlToken::Continue)
            }
        },
        State::DoctypePublicIdentifierSingleQuoted => match slf.read_char()? {
            Some('\'') => {
                slf.state = State::AfterDoctypePublicIdentifier;
                Ok(ControlToken::Continue)
            }
            Some('\0') => {
                slf.emit_error(Error::UnexpectedNullCharacter);
                slf.emitter.push_doctype_public_identifier("\u{fffd}");
                Ok(ControlToken::Continue)
            }
            Some('>') => {
                slf.emit_error(Error::AbruptDoctypePublicIdentifier);
                slf.emitter.set_force_quirks();
                slf.state = State::Data;
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInDoctype);
                slf.emitter.set_force_quirks();
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Eof)
            }
            Some(x) => {
                slf.emitter.push_doctype_public_identifier(ctostr!(x));
                Ok(ControlToken::Continue)
            }
        },
        State::AfterDoctypePublicIdentifier => match slf.read_char()? {
            Some(whitespace_pat!()) => {
                slf.state = State::BetweenDoctypePublicAndSystemIdentifiers;
                Ok(ControlToken::Continue)
            }
            Some('>') => {
                slf.state = State::Data;
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Continue)
            }
            Some('"') => {
                slf.emit_error(Error::MissingWhitespaceBetweenDoctypePublicAndSystemIdentifiers);
                slf.emitter.set_doctype_system_identifier("");
                slf.state = State::DoctypeSystemIdentifierDoubleQuoted;
                Ok(ControlToken::Continue)
            }
            Some('\'') => {
                slf.emit_error(Error::MissingWhitespaceBetweenDoctypePublicAndSystemIdentifiers);
                slf.emitter.set_doctype_system_identifier("");
                slf.state = State::DoctypeSystemIdentifierSingleQuoted;
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInDoctype);
                slf.emitter.set_force_quirks();
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Eof)
            }
            c @ Some(_) => {
                slf.emit_error(Error::MissingQuoteBeforeDoctypeSystemIdentifier);
                slf.emitter.set_force_quirks();
                slf.unread_char(c);
                slf.state = State::BogusDoctype;
                Ok(ControlToken::Continue)
            }
        },
        State::BetweenDoctypePublicAndSystemIdentifiers => match slf.read_char()? {
            Some(whitespace_pat!()) => Ok(ControlToken::Continue),
            Some('>') => {
                slf.state = State::Data;
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Continue)
            }
            Some('"') => {
                slf.emitter.set_doctype_system_identifier("");
                slf.state = State::DoctypeSystemIdentifierDoubleQuoted;
                Ok(ControlToken::Continue)
            }
            Some('\'') => {
                slf.emitter.set_doctype_system_identifier("");
                slf.state = State::DoctypeSystemIdentifierSingleQuoted;
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInDoctype);
                slf.emitter.set_force_quirks();
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Eof)
            }
            c @ Some(_) => {
                slf.emit_error(Error::MissingQuoteBeforeDoctypeSystemIdentifier);
                slf.emitter.set_force_quirks();
                slf.state = State::BogusDoctype;
                slf.unread_char(c);
                Ok(ControlToken::Continue)
            }
        },
        State::AfterDoctypeSystemKeyword => match slf.read_char()? {
            Some(whitespace_pat!()) => {
                slf.state = State::BeforeDoctypeSystemIdentifier;
                Ok(ControlToken::Continue)
            }
            Some('"') => {
                slf.emit_error(Error::MissingWhitespaceAfterDoctypeSystemKeyword);
                slf.emitter.set_doctype_system_identifier("");
                slf.state = State::DoctypeSystemIdentifierDoubleQuoted;
                Ok(ControlToken::Continue)
            }
            Some('\'') => {
                slf.emit_error(Error::MissingWhitespaceAfterDoctypeSystemKeyword);
                slf.emitter.set_doctype_system_identifier("");
                slf.state = State::DoctypeSystemIdentifierSingleQuoted;
                Ok(ControlToken::Continue)
            }
            Some('>') => {
                slf.emit_error(Error::MissingDoctypeSystemIdentifier);
                slf.emitter.set_force_quirks();
                slf.state = State::Data;
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInDoctype);
                slf.emitter.set_force_quirks();
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Eof)
            }
            c @ Some(_) => {
                slf.emit_error(Error::MissingQuoteBeforeDoctypeSystemIdentifier);
                slf.emitter.set_force_quirks();
                slf.state = State::BogusDoctype;
                slf.unread_char(c);
                Ok(ControlToken::Continue)
            }
        },
        State::BeforeDoctypeSystemIdentifier => match slf.read_char()? {
            Some(whitespace_pat!()) => Ok(ControlToken::Continue),
            Some('"') => {
                slf.emitter.set_doctype_system_identifier("");
                slf.state = State::DoctypeSystemIdentifierDoubleQuoted;
                Ok(ControlToken::Continue)
            }
            Some('\'') => {
                slf.emitter.set_doctype_system_identifier("");
                slf.state = State::DoctypeSystemIdentifierSingleQuoted;
                Ok(ControlToken::Continue)
            }
            Some('>') => {
                slf.emit_error(Error::MissingDoctypeSystemIdentifier);
                slf.emitter.set_force_quirks();
                slf.state = State::Data;
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInDoctype);
                slf.emitter.set_force_quirks();
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Eof)
            }
            c @ Some(_) => {
                slf.emit_error(Error::MissingQuoteBeforeDoctypeSystemIdentifier);
                slf.emitter.set_force_quirks();
                slf.state = State::BogusDoctype;
                slf.unread_char(c);
                Ok(ControlToken::Continue)
            }
        },
        State::DoctypeSystemIdentifierDoubleQuoted => match slf.read_char()? {
            Some('"') => {
                slf.state = State::AfterDoctypeSystemIdentifier;
                Ok(ControlToken::Continue)
            }
            Some('\0') => {
                slf.emit_error(Error::UnexpectedNullCharacter);
                slf.emitter.push_doctype_system_identifier("\u{fffd}");
                Ok(ControlToken::Continue)
            }
            Some('>') => {
                slf.emit_error(Error::AbruptDoctypeSystemIdentifier);
                slf.emitter.set_force_quirks();
                slf.state = State::Data;
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInDoctype);
                slf.emitter.set_force_quirks();
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Eof)
            }
            Some(x) => {
                slf.emitter.push_doctype_system_identifier(ctostr!(x));
                Ok(ControlToken::Continue)
            }
        },
        State::DoctypeSystemIdentifierSingleQuoted => match slf.read_char()? {
            Some('\'') => {
                slf.state = State::AfterDoctypeSystemIdentifier;
                Ok(ControlToken::Continue)
            }
            Some('\0') => {
                slf.emit_error(Error::UnexpectedNullCharacter);
                slf.emitter.push_doctype_system_identifier("\u{fffd}");
                Ok(ControlToken::Continue)
            }
            Some('>') => {
                slf.emit_error(Error::AbruptDoctypeSystemIdentifier);
                slf.emitter.set_force_quirks();
                slf.state = State::Data;
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInDoctype);
                slf.emitter.set_force_quirks();
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Eof)
            }
            Some(x) => {
                slf.emitter.push_doctype_system_identifier(ctostr!(x));
                Ok(ControlToken::Continue)
            }
        },
        State::AfterDoctypeSystemIdentifier => match slf.read_char()? {
            Some(whitespace_pat!()) => Ok(ControlToken::Continue),
            Some('>') => {
                slf.state = State::Data;
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInDoctype);
                slf.emitter.set_force_quirks();
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Eof)
            }
            c @ Some(_) => {
                slf.emit_error(Error::UnexpectedCharacterAfterDoctypeSystemIdentifier);
                slf.unread_char(c);
                slf.state = State::BogusDoctype;
                Ok(ControlToken::Continue)
            }
        },
        State::BogusDoctype => match slf.read_char()? {
            Some('>') => {
                slf.state = State::Data;
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Continue)
            }
            Some('\0') => {
                slf.emit_error(Error::UnexpectedNullCharacter);
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emitter.emit_current_doctype();
                Ok(ControlToken::Eof)
            }
            Some(_) => Ok(ControlToken::Continue),
        },
        State::CdataSection => match slf.read_char()? {
            Some(']') => {
                slf.state = State::CdataSectionBracket;
                Ok(ControlToken::Continue)
            }
            None => {
                slf.emit_error(Error::EofInCdata);
                Ok(ControlToken::Eof)
            }
            Some(x) => {
                slf.emitter.emit_string(ctostr!(x));
                Ok(ControlToken::Continue)
            }
        },
        State::CdataSectionBracket => match slf.read_char()? {
            Some(']') => {
                slf.state = State::CdataSectionEnd;
                Ok(ControlToken::Continue)
            }
            c => {
                slf.emitter.emit_string("]");
                slf.state = State::CdataSection;
                slf.unread_char(c);
                Ok(ControlToken::Continue)
            }
        },
        State::CdataSectionEnd => match slf.read_char()? {
            Some(']') => {
                slf.emitter.emit_string("]");
                Ok(ControlToken::Continue)
            }
            Some('>') => {
                slf.state = State::Data;
                Ok(ControlToken::Continue)
            }
            c => {
                slf.emitter.emit_string("]]");
                slf.unread_char(c);
                slf.state = State::CdataSection;
                Ok(ControlToken::Continue)
            }
        },
        State::CharacterReference => {
            slf.temporary_buffer.clear();
            slf.temporary_buffer.push('&');
            match slf.read_char()? {
                Some(x) if x.is_ascii_alphanumeric() => {
                    slf.unread_char(Some(x));
                    slf.state = State::NamedCharacterReference;
                    Ok(ControlToken::Continue)
                }
                Some('#') => {
                    slf.temporary_buffer.push('#');
                    slf.state = State::NumericCharacterReference;
                    Ok(ControlToken::Continue)
                }
                c => {
                    slf.flush_code_points_consumed_as_character_reference();
                    slf.state = slf.return_state.take().unwrap();
                    slf.unread_char(c);
                    Ok(ControlToken::Continue)
                }
            }
        }
        State::NamedCharacterReference => {
            let c = slf.read_char()?;

            let char_ref = match c {
                Some(x) => try_read_character_reference(x, |x| slf.try_read_string(x, true))?
                    .map(|char_ref| (x, char_ref)),

                None => None,
            };

            if let Some((x, char_ref)) = char_ref {
                slf.temporary_buffer.push(x);
                slf.temporary_buffer.push_str(char_ref.name);
                let char_ref_name_last_character = char_ref.name.chars().last();
                let next_character = slf.next_input_character()?;
                if slf.is_consumed_as_part_of_an_attribute()
                    && char_ref_name_last_character != Some(';')
                    && matches!(next_character, Some(x) if x == '=' || x.is_ascii_alphanumeric())
                {
                    slf.flush_code_points_consumed_as_character_reference();
                    slf.state = slf.return_state.take().unwrap();
                    Ok(ControlToken::Continue)
                } else {
                    if char_ref_name_last_character != Some(';') {
                        slf.emit_error(Error::MissingSemicolonAfterCharacterReference);
                    }

                    slf.temporary_buffer.clear();
                    slf.temporary_buffer.push_str(char_ref.characters);
                    slf.flush_code_points_consumed_as_character_reference();
                    slf.state = slf.return_state.take().unwrap();
                    Ok(ControlToken::Continue)
                }
            } else {
                slf.unread_char(c);
                slf.flush_code_points_consumed_as_character_reference();
                slf.state = State::AmbiguousAmpersand;
                Ok(ControlToken::Continue)
            }
        }
        State::AmbiguousAmpersand => match slf.read_char()? {
            Some(x) if x.is_ascii_alphanumeric() => {
                if slf.is_consumed_as_part_of_an_attribute() {
                    slf.emitter.push_attribute_value(ctostr!(x));
                } else {
                    slf.emitter.emit_string(ctostr!(x));
                }

                Ok(ControlToken::Continue)
            }
            c @ Some(';') => {
                slf.emit_error(Error::UnknownNamedCharacterReference);
                slf.unread_char(c);
                slf.state = slf.return_state.take().unwrap();
                Ok(ControlToken::Continue)
            }
            c => {
                slf.unread_char(c);
                slf.state = slf.return_state.take().unwrap();
                Ok(ControlToken::Continue)
            }
        },
        State::NumericCharacterReference => {
            slf.character_reference_code = 0;
            match slf.read_char()? {
                Some(x @ 'x' | x @ 'X') => {
                    slf.temporary_buffer.push(x);
                    slf.state = State::HexadecimalCharacterReferenceStart;
                    Ok(ControlToken::Continue)
                }
                c => {
                    slf.unread_char(c);
                    slf.state = State::DecimalCharacterReferenceStart;
                    Ok(ControlToken::Continue)
                }
            }
        }
        State::HexadecimalCharacterReferenceStart => match slf.read_char()? {
            c @ Some('0'..='9' | 'A'..='F' | 'a'..='f') => {
                slf.unread_char(c);
                slf.state = State::HexadecimalCharacterReference;
                Ok(ControlToken::Continue)
            }
            c => {
                slf.emit_error(Error::AbsenceOfDigitsInNumericCharacterReference);
                slf.flush_code_points_consumed_as_character_reference();
                slf.unread_char(c);
                slf.state = slf.return_state.take().unwrap();
                Ok(ControlToken::Continue)
            }
        },
        State::DecimalCharacterReferenceStart => match slf.read_char()? {
            Some(x @ ascii_digit_pat!()) => {
                slf.unread_char(Some(x));
                slf.state = State::DecimalCharacterReference;
                Ok(ControlToken::Continue)
            }
            c => {
                slf.emit_error(Error::AbsenceOfDigitsInNumericCharacterReference);
                slf.flush_code_points_consumed_as_character_reference();
                slf.unread_char(c);
                slf.state = slf.return_state.take().unwrap();
                Ok(ControlToken::Continue)
            }
        },
        State::HexadecimalCharacterReference => match slf.read_char()? {
            Some(x @ ascii_digit_pat!()) => {
                mutate_character_reference!(*16 + x - 0x0030);
                Ok(ControlToken::Continue)
            }
            Some(x @ 'A'..='F') => {
                mutate_character_reference!(*16 + x - 0x0037);
                Ok(ControlToken::Continue)
            }
            Some(x @ 'a'..='f') => {
                mutate_character_reference!(*16 + x - 0x0057);
                Ok(ControlToken::Continue)
            }
            Some(';') => {
                slf.state = State::NumericCharacterReferenceEnd;
                Ok(ControlToken::Continue)
            }
            c => {
                slf.emit_error(Error::MissingSemicolonAfterCharacterReference);
                slf.unread_char(c);
                slf.state = State::NumericCharacterReferenceEnd;
                Ok(ControlToken::Continue)
            }
        },
        State::DecimalCharacterReference => match slf.read_char()? {
            Some(x @ ascii_digit_pat!()) => {
                mutate_character_reference!(*10 + x - 0x0030);
                Ok(ControlToken::Continue)
            }
            Some(';') => {
                slf.state = State::NumericCharacterReferenceEnd;
                Ok(ControlToken::Continue)
            }
            c => {
                slf.emit_error(Error::MissingSemicolonAfterCharacterReference);
                slf.unread_char(c);
                slf.state = State::NumericCharacterReferenceEnd;
                Ok(ControlToken::Continue)
            }
        },
        State::NumericCharacterReferenceEnd => {
            match slf.character_reference_code {
                0x00 => {
                    slf.emit_error(Error::NullCharacterReference);
                    slf.character_reference_code = 0xfffd;
                }
                0x110000.. => {
                    slf.emit_error(Error::CharacterReferenceOutsideUnicodeRange);
                    slf.character_reference_code = 0xfffd;
                }
                surrogate_pat!() => {
                    slf.emit_error(Error::SurrogateCharacterReference);
                    slf.character_reference_code = 0xfffd;
                }
                // noncharacter
                noncharacter_pat!() => {
                    slf.emit_error(Error::NoncharacterCharacterReference);
                }
                // 0x000d, or a control that is not whitespace
                x @ 0x000d | x @ control_pat!()
                    if !matches!(x, 0x0009 | 0x000a | 0x000c | 0x0020) =>
                {
                    slf.emit_error(Error::ControlCharacterReference);
                    slf.character_reference_code = match x {
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
                        _ => slf.character_reference_code,
                    };
                }
                _ => (),
            }

            slf.temporary_buffer.clear();
            slf.temporary_buffer
                .push(std::char::from_u32(slf.character_reference_code).unwrap());
            slf.flush_code_points_consumed_as_character_reference();
            slf.state = slf.return_state.take().unwrap();
            Ok(ControlToken::Continue)
        }
    }
}
