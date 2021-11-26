#![deny(missing_docs)]
// This is an HTML parser. HTML can be untrusted input from the internet.
#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod emitter;
mod entities;
mod error;
mod machine;
mod never;
mod reader;

#[cfg(feature = "integration-tests")]
pub use machine::State;
#[cfg(not(feature = "integration-tests"))]
use machine::State;

use machine::{
    ascii_digit_pat, control_pat, noncharacter_pat, surrogate_pat, whitespace_pat, ControlToken,
};

pub use emitter::{DefaultEmitter, Doctype, Emitter, EndTag, StartTag, Token};
pub use error::Error;
pub use never::Never;
pub use reader::{BufReadReader, Readable, Reader, StringReader};

macro_rules! ctostr {
    ($c:expr) => {
        &*$c.encode_utf8(&mut [0; 4])
    };
}

// this is a stack that can hold 0 to 2 Ts
#[derive(Debug, Default)]
struct Stack2<T: Copy>(Option<(T, Option<T>)>);

impl<T: Copy> Stack2<T> {
    #[inline]
    fn push(&mut self, c: T) {
        self.0 = match self.0 {
            None => Some((c, None)),
            Some((c1, None)) => Some((c1, Some(c))),
            Some((_c1, Some(_c2))) => panic!("stack full!"),
        }
    }

    #[inline]
    fn pop(&mut self) -> Option<T> {
        let (new_self, rv) = match self.0 {
            Some((c1, Some(c2))) => (Some((c1, None)), Some(c2)),
            Some((c1, None)) => (None, Some(c1)),
            None => (None, None),
        };
        self.0 = new_self;
        rv
    }

    #[inline]
    fn is_empty(&self) -> bool {
        matches!(self.0, None)
    }
}

/// A HTML tokenizer. See crate-level docs for basic usage.
pub struct Tokenizer<R: Reader, E: Emitter = DefaultEmitter> {
    eof: bool,
    state: State,
    emitter: E,
    temporary_buffer: String,
    reader: R,
    to_reconsume: Stack2<Option<char>>,
    character_reference_code: u32,
    return_state: Option<State>,
}

impl<R: Reader> Tokenizer<R> {
    /// Create a new tokenizer from some input.
    ///
    /// `input` can be `&String` or `&str` at the moment, as those are the types for which
    /// [`crate::Readable`] is implemented, but you can implement that trait on your own types.
    ///
    /// Patches are welcome for providing an efficient implementation over async streams,
    /// iterators, files, etc, as long as any dependencies come behind featureflags.
    pub fn new<'a, S: Readable<'a, Reader = R>>(input: S) -> Self {
        Tokenizer::<S::Reader>::new_with_emitter(input, DefaultEmitter::default())
    }
}

impl<R: Reader, E: Emitter> Tokenizer<R, E> {
    /// Construct a new tokenizer from some input and a custom emitter.
    ///
    /// Use this method over [`Tokenizer::new`] when you want to have more control over string allocation for
    /// tokens.
    pub fn new_with_emitter<'a, S: Readable<'a, Reader = R>>(input: S, emitter: E) -> Self {
        Tokenizer {
            eof: false,
            state: State::Data,
            emitter,
            temporary_buffer: String::new(),
            to_reconsume: Stack2::default(),
            reader: input.to_reader(),
            character_reference_code: 0,
            return_state: None,
        }
    }

    #[cfg(feature = "integration-tests")]
    /// Test-internal function to override internal state.
    ///
    /// Only available with the `integration-tests` feature which is not public API.
    pub fn set_state(&mut self, state: State) {
        self.state = state;
    }

    /// Set the statemachine to start/continue in [plaintext
    /// state](https://html.spec.whatwg.org/#plaintext-state).
    ///
    /// This tokenizer never gets into that state naturally.
    pub fn set_plaintext_state(&mut self) {
        self.state = State::PlainText;
    }

    #[cfg(feature = "integration-tests")]
    /// Test-internal function to override internal state.
    ///
    /// Only available with the `integration-tests` feature which is not public API.
    pub fn set_last_start_tag(&mut self, last_start_tag: Option<&str>) {
        self.emitter.set_last_start_tag(last_start_tag);
    }

    #[inline]
    fn unread_char(&mut self, c: Option<char>) {
        self.to_reconsume.push(c);
    }

    #[inline]
    fn validate_char(&mut self, c: char) {
        match c as u32 {
            surrogate_pat!() => {
                self.emitter.emit_error(Error::SurrogateInInputStream);
            }
            noncharacter_pat!() => {
                self.emitter.emit_error(Error::NoncharacterInInputStream);
            }
            // control without whitespace or nul
            x @ control_pat!()
                if !matches!(x, 0x0000 | 0x0009 | 0x000a | 0x000c | 0x000d | 0x0020) =>
            {
                self.emitter
                    .emit_error(Error::ControlCharacterInInputStream);
            }
            _ => (),
        }
    }

    fn read_char(&mut self) -> Result<Option<char>, R::Error> {
        let (c_res, reconsumed) = match self.to_reconsume.pop() {
            Some(c) => (Ok(c), true),
            None => (self.reader.read_char(), false),
        };

        let mut c = match c_res {
            Ok(Some(c)) => c,
            res => return res,
        };

        if c == '\r' {
            c = '\n';
            let c2 = self.reader.read_char()?;
            if c2 != Some('\n') {
                self.unread_char(c2);
            }
        }

        if !reconsumed {
            self.validate_char(c);
        }

        Ok(Some(c))
    }

    #[inline]
    fn try_read_string(&mut self, s: &str, case_sensitive: bool) -> Result<bool, R::Error> {
        debug_assert!(!s.is_empty());
        debug_assert!(self.to_reconsume.is_empty());
        self.reader.try_read_string(s, case_sensitive)
    }

    fn is_consumed_as_part_of_an_attribute(&self) -> bool {
        matches!(
            self.return_state,
            Some(
                State::AttributeValueDoubleQuoted
                    | State::AttributeValueSingleQuoted
                    | State::AttributeValueUnquoted
            )
        )
    }

    fn flush_code_points_consumed_as_character_reference(&mut self) {
        if self.is_consumed_as_part_of_an_attribute() {
            self.emitter.push_attribute_value(&self.temporary_buffer);
            self.temporary_buffer.clear();
        } else {
            self.flush_buffer_characters();
        }
    }

    fn next_input_character(&mut self) -> Result<Option<char>, R::Error> {
        let rv = self.read_char()?;
        self.unread_char(rv);
        Ok(rv)
    }

    fn flush_buffer_characters(&mut self) {
        self.emitter.emit_string(&self.temporary_buffer);
        self.temporary_buffer.clear();
    }

    fn consume(&mut self) -> Result<ControlToken, R::Error> {
        macro_rules! mutate_character_reference {
            (* $mul:literal + $x:ident - $sub:literal) => {
                match self
                    .character_reference_code
                    .checked_mul($mul)
                    .and_then(|cr| cr.checked_add($x as u32 - $sub))
                {
                    Some(cr) => self.character_reference_code = cr,
                    None => {
                        // provoke err
                        self.character_reference_code = 0x110000;
                    }
                };
            };
        }

        match self.state {
            State::Data => match self.read_char()? {
                Some('&') => {
                    self.return_state = Some(self.state);
                    self.state = State::CharacterReference;
                    Ok(ControlToken::Continue)
                }
                Some('<') => {
                    self.state = State::TagOpen;
                    Ok(ControlToken::Continue)
                }
                Some('\0') => {
                    self.emitter.emit_error(Error::UnexpectedNullCharacter);
                    self.emitter.emit_string("\0");
                    Ok(ControlToken::Continue)
                }
                Some(x) => {
                    self.emitter.emit_string(ctostr!(x));
                    Ok(ControlToken::Continue)
                }
                None => Ok(ControlToken::Eof),
            },
            State::RcData => match self.read_char()? {
                Some('&') => {
                    self.return_state = Some(State::RcData);
                    self.state = State::CharacterReference;
                    Ok(ControlToken::Continue)
                }
                Some('<') => {
                    self.state = State::RcDataLessThanSign;
                    Ok(ControlToken::Continue)
                }
                Some('\0') => {
                    self.emitter.emit_error(Error::UnexpectedNullCharacter);
                    self.emitter.emit_string("\u{fffd}");
                    Ok(ControlToken::Continue)
                }
                Some(x) => {
                    self.emitter.emit_string(ctostr!(x));
                    Ok(ControlToken::Continue)
                }
                None => Ok(ControlToken::Eof),
            },
            State::RawText => match self.read_char()? {
                Some('<') => {
                    self.state = State::RawTextLessThanSign;
                    Ok(ControlToken::Continue)
                }
                Some('\0') => {
                    self.emitter.emit_error(Error::UnexpectedNullCharacter);
                    self.emitter.emit_string("\u{fffd}");
                    Ok(ControlToken::Continue)
                }
                Some(x) => {
                    self.emitter.emit_string(ctostr!(x));
                    Ok(ControlToken::Continue)
                }
                None => Ok(ControlToken::Eof),
            },
            State::ScriptData => match self.read_char()? {
                Some('<') => {
                    self.state = State::ScriptDataLessThanSign;
                    Ok(ControlToken::Continue)
                }
                Some('\0') => {
                    self.emitter.emit_error(Error::UnexpectedNullCharacter);
                    self.emitter.emit_string("\u{fffd}");
                    Ok(ControlToken::Continue)
                }
                Some(x) => {
                    self.emitter.emit_string(ctostr!(x));
                    Ok(ControlToken::Continue)
                }
                None => Ok(ControlToken::Eof),
            },
            State::PlainText => match self.read_char()? {
                Some('\0') => {
                    self.emitter.emit_error(Error::UnexpectedNullCharacter);
                    self.emitter.emit_string("\u{fffd}");
                    Ok(ControlToken::Continue)
                }
                Some(x) => {
                    self.emitter.emit_string(ctostr!(x));
                    Ok(ControlToken::Continue)
                }
                None => Ok(ControlToken::Eof),
            },
            State::TagOpen => match self.read_char()? {
                Some('!') => {
                    self.state = State::MarkupDeclarationOpen;
                    Ok(ControlToken::Continue)
                }
                Some('/') => {
                    self.state = State::EndTagOpen;
                    Ok(ControlToken::Continue)
                }
                Some(x) if x.is_ascii_alphabetic() => {
                    self.emitter.init_start_tag();
                    self.state = State::TagName;
                    self.unread_char(Some(x));
                    Ok(ControlToken::Continue)
                }
                c @ Some('?') => {
                    self.emitter
                        .emit_error(Error::UnexpectedQuestionMarkInsteadOfTagName);
                    self.emitter.init_comment();
                    self.state = State::BogusComment;
                    self.unread_char(c);
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_error(Error::EofBeforeTagName);
                    self.emitter.emit_string("<");
                    Ok(ControlToken::Eof)
                }
                c @ Some(_) => {
                    self.emitter
                        .emit_error(Error::InvalidFirstCharacterOfTagName);
                    self.state = State::Data;
                    self.emitter.emit_string("<");
                    self.unread_char(c);
                    Ok(ControlToken::Continue)
                }
            },
            State::EndTagOpen => match self.read_char()? {
                Some(x) if x.is_ascii_alphabetic() => {
                    self.emitter.init_end_tag();
                    self.state = State::TagName;
                    self.unread_char(Some(x));
                    Ok(ControlToken::Continue)
                }
                Some('>') => {
                    self.emitter.emit_error(Error::MissingEndTagName);
                    self.state = State::Data;
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_error(Error::EofBeforeTagName);
                    self.emitter.emit_string("</");
                    Ok(ControlToken::Eof)
                }
                Some(x) => {
                    self.emitter
                        .emit_error(Error::InvalidFirstCharacterOfTagName);
                    self.emitter.init_comment();
                    self.state = State::BogusComment;
                    self.unread_char(Some(x));
                    Ok(ControlToken::Continue)
                }
            },
            State::TagName => match self.read_char()? {
                Some(whitespace_pat!()) => {
                    self.state = State::BeforeAttributeName;
                    Ok(ControlToken::Continue)
                }
                Some('/') => {
                    self.state = State::SelfClosingStartTag;
                    Ok(ControlToken::Continue)
                }
                Some('>') => {
                    self.state = State::Data;
                    self.emitter.emit_current_tag();
                    Ok(ControlToken::Continue)
                }
                Some('\0') => {
                    self.emitter.emit_error(Error::UnexpectedNullCharacter);
                    self.emitter.push_tag_name("\u{fffd}");
                    Ok(ControlToken::Continue)
                }
                Some(x) => {
                    self.emitter.push_tag_name(ctostr!(x.to_ascii_lowercase()));
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_error(Error::EofInTag);
                    Ok(ControlToken::Eof)
                }
            },
            State::RcDataLessThanSign => match self.read_char()? {
                Some('/') => {
                    self.temporary_buffer.clear();
                    self.state = State::RcDataEndTagOpen;
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.emitter.emit_string("<");
                    self.state = State::RcData;
                    self.unread_char(c);
                    Ok(ControlToken::Continue)
                }
            },
            State::RcDataEndTagOpen => match self.read_char()? {
                Some(x) if x.is_ascii_alphabetic() => {
                    self.emitter.init_end_tag();
                    self.state = State::RcDataEndTagName;
                    self.unread_char(Some(x));
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.emitter.emit_string("</");
                    self.state = State::RcData;
                    self.unread_char(c);
                    Ok(ControlToken::Continue)
                }
            },
            State::RcDataEndTagName => match self.read_char()? {
                Some(whitespace_pat!()) if self.emitter.current_is_appropriate_end_tag_token() => {
                    self.state = State::BeforeAttributeName;
                    Ok(ControlToken::Continue)
                }
                Some('/') if self.emitter.current_is_appropriate_end_tag_token() => {
                    self.state = State::SelfClosingStartTag;
                    Ok(ControlToken::Continue)
                }
                Some('>') if self.emitter.current_is_appropriate_end_tag_token() => {
                    self.state = State::Data;
                    self.emitter.emit_current_tag();
                    Ok(ControlToken::Continue)
                }
                Some(x) if x.is_ascii_alphabetic() => {
                    self.emitter.push_tag_name(ctostr!(x.to_ascii_lowercase()));
                    self.temporary_buffer.push(x);
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.emitter.emit_string("</");
                    self.flush_buffer_characters();

                    self.state = State::RcData;
                    self.unread_char(c);
                    Ok(ControlToken::Continue)
                }
            },
            State::RawTextLessThanSign => match self.read_char()? {
                Some('/') => {
                    self.temporary_buffer.clear();
                    self.state = State::RawTextEndTagOpen;
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.emitter.emit_string("<");
                    self.state = State::RawText;
                    self.unread_char(c);
                    Ok(ControlToken::Continue)
                }
            },
            State::RawTextEndTagOpen => match self.read_char()? {
                Some(x) if x.is_ascii_alphabetic() => {
                    self.emitter.init_end_tag();
                    self.state = State::RawTextEndTagName;
                    self.unread_char(Some(x));
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.emitter.emit_string("</");
                    self.state = State::RawText;
                    self.unread_char(c);
                    Ok(ControlToken::Continue)
                }
            },
            State::RawTextEndTagName => match self.read_char()? {
                Some(whitespace_pat!()) if self.emitter.current_is_appropriate_end_tag_token() => {
                    self.state = State::BeforeAttributeName;
                    Ok(ControlToken::Continue)
                }
                Some('/') if self.emitter.current_is_appropriate_end_tag_token() => {
                    self.state = State::SelfClosingStartTag;
                    Ok(ControlToken::Continue)
                }
                Some('>') if self.emitter.current_is_appropriate_end_tag_token() => {
                    self.state = State::Data;
                    self.emitter.emit_current_tag();
                    Ok(ControlToken::Continue)
                }
                Some(x) if x.is_ascii_alphabetic() => {
                    self.emitter.push_tag_name(ctostr!(x.to_ascii_lowercase()));
                    self.temporary_buffer.push(x);
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.emitter.emit_string("</");
                    self.flush_buffer_characters();

                    self.state = State::RawText;
                    self.unread_char(c);
                    Ok(ControlToken::Continue)
                }
            },
            State::ScriptDataLessThanSign => match self.read_char()? {
                Some('/') => {
                    self.temporary_buffer.clear();
                    self.state = State::ScriptDataEndTagOpen;
                    Ok(ControlToken::Continue)
                }
                Some('!') => {
                    self.state = State::ScriptDataEscapeStart;
                    self.emitter.emit_string("<!");
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.emitter.emit_string("<");
                    self.state = State::Data;
                    self.unread_char(c);
                    Ok(ControlToken::Continue)
                }
            },
            State::ScriptDataEndTagOpen => match self.read_char()? {
                Some(x) if x.is_ascii_alphabetic() => {
                    self.emitter.init_end_tag();
                    self.state = State::ScriptDataEndTagName;
                    self.unread_char(Some(x));
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.emitter.emit_string("</");
                    self.state = State::ScriptData;
                    self.unread_char(c);
                    Ok(ControlToken::Continue)
                }
            },
            State::ScriptDataEndTagName => match self.read_char()? {
                Some(whitespace_pat!()) if self.emitter.current_is_appropriate_end_tag_token() => {
                    self.state = State::BeforeAttributeName;
                    Ok(ControlToken::Continue)
                }
                Some('/') if self.emitter.current_is_appropriate_end_tag_token() => {
                    self.state = State::SelfClosingStartTag;
                    Ok(ControlToken::Continue)
                }
                Some('>') if self.emitter.current_is_appropriate_end_tag_token() => {
                    self.state = State::Data;
                    self.emitter.emit_current_tag();
                    Ok(ControlToken::Continue)
                }
                Some(x) if x.is_ascii_alphabetic() => {
                    self.emitter.push_tag_name(ctostr!(x.to_ascii_lowercase()));
                    self.temporary_buffer.push(x.to_ascii_lowercase());
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.emitter.emit_string("</");
                    self.flush_buffer_characters();
                    self.state = State::Data;
                    self.unread_char(c);
                    Ok(ControlToken::Continue)
                }
            },
            State::ScriptDataEscapeStart => match self.read_char()? {
                Some('-') => {
                    self.state = State::ScriptDataEscapeStartDash;
                    self.emitter.emit_string("-");
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.state = State::ScriptData;
                    self.unread_char(c);
                    Ok(ControlToken::Continue)
                }
            },
            State::ScriptDataEscapeStartDash => match self.read_char()? {
                Some('-') => {
                    self.state = State::ScriptDataEscapedDashDash;
                    self.emitter.emit_string("-");
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.state = State::ScriptData;
                    self.unread_char(c);
                    Ok(ControlToken::Continue)
                }
            },
            State::ScriptDataEscaped => match self.read_char()? {
                Some('-') => {
                    self.state = State::ScriptDataEscapedDash;
                    self.emitter.emit_string("-");
                    Ok(ControlToken::Continue)
                }
                Some('<') => {
                    self.state = State::ScriptDataEscapedLessThanSign;
                    Ok(ControlToken::Continue)
                }
                Some('\0') => {
                    self.emitter.emit_error(Error::UnexpectedNullCharacter);
                    self.emitter.emit_string("\u{fffd}");
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter
                        .emit_error(Error::EofInScriptHtmlCommentLikeText);
                    Ok(ControlToken::Eof)
                }
                Some(x) => {
                    self.emitter.emit_string(ctostr!(x));
                    Ok(ControlToken::Continue)
                }
            },
            State::ScriptDataEscapedDash => match self.read_char()? {
                Some('-') => {
                    self.state = State::ScriptDataEscapedDashDash;
                    self.emitter.emit_string("-");
                    Ok(ControlToken::Continue)
                }
                Some('<') => {
                    self.state = State::ScriptDataEscapedLessThanSign;
                    Ok(ControlToken::Continue)
                }
                Some('\0') => {
                    self.emitter.emit_error(Error::UnexpectedNullCharacter);
                    self.state = State::ScriptDataEscaped;
                    self.emitter.emit_string("\u{fffd}");
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter
                        .emit_error(Error::EofInScriptHtmlCommentLikeText);
                    Ok(ControlToken::Eof)
                }
                Some(x) => {
                    self.state = State::ScriptDataEscaped;
                    self.emitter.emit_string(ctostr!(x));
                    Ok(ControlToken::Continue)
                }
            },
            State::ScriptDataEscapedDashDash => match self.read_char()? {
                Some('-') => {
                    self.emitter.emit_string("-");
                    Ok(ControlToken::Continue)
                }
                Some('<') => {
                    self.state = State::ScriptDataEscapedLessThanSign;
                    Ok(ControlToken::Continue)
                }
                Some('>') => {
                    self.state = State::ScriptData;
                    self.emitter.emit_string(">");
                    Ok(ControlToken::Continue)
                }
                Some('\0') => {
                    self.emitter.emit_error(Error::UnexpectedNullCharacter);
                    self.state = State::ScriptDataEscaped;
                    self.emitter.emit_string("\u{fffd}");
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter
                        .emit_error(Error::EofInScriptHtmlCommentLikeText);
                    Ok(ControlToken::Eof)
                }
                Some(x) => {
                    self.state = State::ScriptDataEscaped;
                    self.emitter.emit_string(ctostr!(x));
                    Ok(ControlToken::Continue)
                }
            },
            State::ScriptDataEscapedLessThanSign => match self.read_char()? {
                Some('/') => {
                    self.temporary_buffer.clear();
                    self.state = State::ScriptDataEscapedEndTagOpen;
                    Ok(ControlToken::Continue)
                }
                Some(x) if x.is_ascii_alphabetic() => {
                    self.temporary_buffer.clear();
                    self.emitter.emit_string("<");
                    self.state = State::ScriptDataDoubleEscapeStart;
                    self.unread_char(Some(x));
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.emitter.emit_string("<");
                    self.state = State::ScriptDataEscaped;
                    self.unread_char(c);
                    Ok(ControlToken::Continue)
                }
            },
            State::ScriptDataEscapedEndTagOpen => match self.read_char()? {
                Some(x) if x.is_ascii_alphabetic() => {
                    self.emitter.init_end_tag();
                    self.state = State::ScriptDataEscapedEndTagName;
                    self.unread_char(Some(x));
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.emitter.emit_string("</");
                    self.unread_char(c);
                    self.state = State::ScriptDataEscaped;
                    Ok(ControlToken::Continue)
                }
            },
            State::ScriptDataEscapedEndTagName => match self.read_char()? {
                Some(whitespace_pat!()) if self.emitter.current_is_appropriate_end_tag_token() => {
                    self.state = State::BeforeAttributeName;
                    Ok(ControlToken::Continue)
                }
                Some('/') if self.emitter.current_is_appropriate_end_tag_token() => {
                    self.state = State::SelfClosingStartTag;
                    Ok(ControlToken::Continue)
                }
                Some('>') if self.emitter.current_is_appropriate_end_tag_token() => {
                    self.state = State::Data;
                    self.emitter.emit_current_tag();
                    Ok(ControlToken::Continue)
                }
                Some(x) if x.is_ascii_alphabetic() => {
                    self.emitter.push_tag_name(ctostr!(x.to_ascii_lowercase()));
                    self.temporary_buffer.push(x);
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.emitter.emit_string("</");
                    self.flush_buffer_characters();
                    self.state = State::ScriptDataEscaped;
                    self.unread_char(c);
                    Ok(ControlToken::Continue)
                }
            },
            State::ScriptDataDoubleEscapeStart => match self.read_char()? {
                Some(x @ whitespace_pat!() | x @ '/' | x @ '>') => {
                    if self.temporary_buffer == "script" {
                        self.state = State::ScriptDataDoubleEscaped;
                    } else {
                        self.state = State::ScriptDataEscaped;
                    }
                    self.emitter.emit_string(ctostr!(x));
                    Ok(ControlToken::Continue)
                }
                Some(x) if x.is_ascii_alphabetic() => {
                    self.temporary_buffer.push(x.to_ascii_lowercase());
                    self.emitter.emit_string(ctostr!(x));
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.state = State::ScriptDataEscaped;
                    self.unread_char(c);
                    Ok(ControlToken::Continue)
                }
            },
            State::ScriptDataDoubleEscaped => match self.read_char()? {
                Some('-') => {
                    self.state = State::ScriptDataDoubleEscapedDash;
                    self.emitter.emit_string("-");
                    Ok(ControlToken::Continue)
                }
                Some('<') => {
                    self.state = State::ScriptDataDoubleEscapedLessThanSign;
                    self.emitter.emit_string("<");
                    Ok(ControlToken::Continue)
                }
                Some('\0') => {
                    self.emitter.emit_error(Error::UnexpectedNullCharacter);
                    self.emitter.emit_string("\u{fffd}");
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter
                        .emit_error(Error::EofInScriptHtmlCommentLikeText);
                    Ok(ControlToken::Eof)
                }
                Some(x) => {
                    self.emitter.emit_string(ctostr!(x));
                    Ok(ControlToken::Continue)
                }
            },
            State::ScriptDataDoubleEscapedDash => match self.read_char()? {
                Some('-') => {
                    self.state = State::ScriptDataDoubleEscapedDashDash;
                    self.emitter.emit_string("-");
                    Ok(ControlToken::Continue)
                }
                Some('<') => {
                    self.state = State::ScriptDataDoubleEscapedLessThanSign;
                    self.emitter.emit_string("<");
                    Ok(ControlToken::Continue)
                }
                Some('\0') => {
                    self.emitter.emit_error(Error::UnexpectedNullCharacter);
                    self.state = State::ScriptDataDoubleEscaped;
                    self.emitter.emit_string("\u{fffd}");
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter
                        .emit_error(Error::EofInScriptHtmlCommentLikeText);
                    Ok(ControlToken::Eof)
                }
                Some(x) => {
                    self.state = State::ScriptDataDoubleEscaped;
                    self.emitter.emit_string(ctostr!(x));
                    Ok(ControlToken::Continue)
                }
            },
            State::ScriptDataDoubleEscapedDashDash => match self.read_char()? {
                Some('-') => {
                    self.emitter.emit_string("-");
                    Ok(ControlToken::Continue)
                }
                Some('<') => {
                    self.emitter.emit_string("<");
                    self.state = State::ScriptDataDoubleEscapedLessThanSign;
                    Ok(ControlToken::Continue)
                }
                Some('>') => {
                    self.emitter.emit_string(">");
                    self.state = State::ScriptData;
                    Ok(ControlToken::Continue)
                }
                Some('\0') => {
                    self.emitter.emit_error(Error::UnexpectedNullCharacter);
                    self.state = State::ScriptDataDoubleEscaped;
                    self.emitter.emit_string("\u{fffd}");
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter
                        .emit_error(Error::EofInScriptHtmlCommentLikeText);
                    Ok(ControlToken::Eof)
                }
                Some(x) => {
                    self.state = State::ScriptDataDoubleEscaped;
                    self.emitter.emit_string(ctostr!(x));
                    Ok(ControlToken::Continue)
                }
            },
            State::ScriptDataDoubleEscapedLessThanSign => match self.read_char()? {
                Some('/') => {
                    self.temporary_buffer.clear();
                    self.state = State::ScriptDataDoubleEscapeEnd;
                    self.emitter.emit_string("/");
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.state = State::ScriptDataDoubleEscaped;
                    self.unread_char(c);
                    Ok(ControlToken::Continue)
                }
            },
            State::ScriptDataDoubleEscapeEnd => match self.read_char()? {
                Some(x @ whitespace_pat!() | x @ '/' | x @ '>') => {
                    if self.temporary_buffer == "script" {
                        self.state = State::ScriptDataEscaped;
                    } else {
                        self.state = State::ScriptDataDoubleEscaped;
                    }

                    self.emitter.emit_string(ctostr!(x));
                    Ok(ControlToken::Continue)
                }
                Some(x) if x.is_ascii_alphabetic() => {
                    self.temporary_buffer.push(x.to_ascii_lowercase());
                    self.emitter.emit_string(ctostr!(x));
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.state = State::ScriptDataDoubleEscaped;
                    self.unread_char(c);
                    Ok(ControlToken::Continue)
                }
            },
            State::BeforeAttributeName => match self.read_char()? {
                Some(whitespace_pat!()) => Ok(ControlToken::Continue),
                c @ Some('/' | '>') | c @ None => {
                    self.state = State::AfterAttributeName;
                    self.unread_char(c);
                    Ok(ControlToken::Continue)
                }
                Some('=') => {
                    self.emitter
                        .emit_error(Error::UnexpectedEqualsSignBeforeAttributeName);
                    self.emitter.init_attribute();
                    self.emitter.push_attribute_name("=");
                    self.state = State::AttributeName;
                    Ok(ControlToken::Continue)
                }
                Some(x) => {
                    self.emitter.init_attribute();
                    self.state = State::AttributeName;
                    self.unread_char(Some(x));
                    Ok(ControlToken::Continue)
                }
            },
            State::AttributeName => match self.read_char()? {
                c @ Some(whitespace_pat!() | '/' | '>') | c @ None => {
                    self.state = State::AfterAttributeName;
                    self.unread_char(c);
                    Ok(ControlToken::Continue)
                }
                Some('=') => {
                    self.state = State::BeforeAttributeValue;
                    Ok(ControlToken::Continue)
                }
                Some('\0') => {
                    self.emitter.emit_error(Error::UnexpectedNullCharacter);
                    self.emitter.push_attribute_name("\u{fffd}");
                    Ok(ControlToken::Continue)
                }
                Some(x @ '"' | x @ '\'' | x @ '<') => {
                    self.emitter
                        .emit_error(Error::UnexpectedCharacterInAttributeName);
                    self.emitter
                        .push_attribute_name(ctostr!(x.to_ascii_lowercase()));
                    Ok(ControlToken::Continue)
                }
                Some(x) => {
                    self.emitter
                        .push_attribute_name(ctostr!(x.to_ascii_lowercase()));
                    Ok(ControlToken::Continue)
                }
            },
            State::AfterAttributeName => match self.read_char()? {
                Some(whitespace_pat!()) => Ok(ControlToken::Continue),
                Some('/') => {
                    self.state = State::SelfClosingStartTag;
                    Ok(ControlToken::Continue)
                }
                Some('=') => {
                    self.state = State::BeforeAttributeValue;
                    Ok(ControlToken::Continue)
                }
                Some('>') => {
                    self.state = State::Data;
                    self.emitter.emit_current_tag();
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_error(Error::EofInTag);
                    Ok(ControlToken::Eof)
                }
                Some(x) => {
                    self.emitter.init_attribute();
                    self.state = State::AttributeName;
                    self.unread_char(Some(x));
                    Ok(ControlToken::Continue)
                }
            },
            State::BeforeAttributeValue => match self.read_char()? {
                Some(whitespace_pat!()) => Ok(ControlToken::Continue),
                Some('"') => {
                    self.state = State::AttributeValueDoubleQuoted;
                    Ok(ControlToken::Continue)
                }
                Some('\'') => {
                    self.state = State::AttributeValueSingleQuoted;
                    Ok(ControlToken::Continue)
                }
                Some('>') => {
                    self.emitter.emit_error(Error::MissingAttributeValue);
                    self.state = State::Data;
                    self.emitter.emit_current_tag();
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.state = State::AttributeValueUnquoted;
                    self.unread_char(c);
                    Ok(ControlToken::Continue)
                }
            },
            State::AttributeValueDoubleQuoted => match self.read_char()? {
                Some('"') => {
                    self.state = State::AfterAttributeValueQuoted;
                    Ok(ControlToken::Continue)
                }
                Some('&') => {
                    self.return_state = Some(State::AttributeValueDoubleQuoted);
                    self.state = State::CharacterReference;
                    Ok(ControlToken::Continue)
                }
                Some('\0') => {
                    self.emitter.emit_error(Error::UnexpectedNullCharacter);
                    self.emitter.push_attribute_value("\u{fffd}");
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_error(Error::EofInTag);
                    Ok(ControlToken::Eof)
                }
                Some(x) => {
                    self.emitter.push_attribute_value(ctostr!(x));
                    Ok(ControlToken::Continue)
                }
            },
            State::AttributeValueSingleQuoted => match self.read_char()? {
                Some('\'') => {
                    self.state = State::AfterAttributeValueQuoted;
                    Ok(ControlToken::Continue)
                }
                Some('&') => {
                    self.return_state = Some(State::AttributeValueSingleQuoted);
                    self.state = State::CharacterReference;
                    Ok(ControlToken::Continue)
                }
                Some('\0') => {
                    self.emitter.emit_error(Error::UnexpectedNullCharacter);
                    self.emitter.push_attribute_value("\u{fffd}");
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_error(Error::EofInTag);
                    Ok(ControlToken::Eof)
                }
                Some(x) => {
                    self.emitter.push_attribute_value(ctostr!(x));
                    Ok(ControlToken::Continue)
                }
            },
            State::AttributeValueUnquoted => match self.read_char()? {
                Some(whitespace_pat!()) => {
                    self.state = State::BeforeAttributeName;
                    Ok(ControlToken::Continue)
                }
                Some('&') => {
                    self.return_state = Some(State::AttributeValueUnquoted);
                    self.state = State::CharacterReference;
                    Ok(ControlToken::Continue)
                }
                Some('>') => {
                    self.state = State::Data;
                    self.emitter.emit_current_tag();
                    Ok(ControlToken::Continue)
                }
                Some('\0') => {
                    self.emitter.emit_error(Error::UnexpectedNullCharacter);
                    self.emitter.push_attribute_value("\u{fffd}");
                    Ok(ControlToken::Continue)
                }
                Some(x @ '"' | x @ '\'' | x @ '<' | x @ '=' | x @ '\u{60}') => {
                    self.emitter
                        .emit_error(Error::UnexpectedCharacterInUnquotedAttributeValue);
                    self.emitter.push_attribute_value(ctostr!(x));
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_error(Error::EofInTag);
                    Ok(ControlToken::Eof)
                }
                Some(x) => {
                    self.emitter.push_attribute_value(ctostr!(x));
                    Ok(ControlToken::Continue)
                }
            },
            State::AfterAttributeValueQuoted => match self.read_char()? {
                Some(whitespace_pat!()) => {
                    self.state = State::BeforeAttributeName;
                    Ok(ControlToken::Continue)
                }
                Some('/') => {
                    self.state = State::SelfClosingStartTag;
                    Ok(ControlToken::Continue)
                }
                Some('>') => {
                    self.state = State::Data;
                    self.emitter.emit_current_tag();
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_error(Error::EofInTag);
                    Ok(ControlToken::Eof)
                }
                Some(x) => {
                    self.emitter
                        .emit_error(Error::MissingWhitespaceBetweenAttributes);
                    self.state = State::BeforeAttributeName;
                    self.unread_char(Some(x));
                    Ok(ControlToken::Continue)
                }
            },
            State::SelfClosingStartTag => match self.read_char()? {
                Some('>') => {
                    self.emitter.set_self_closing();
                    self.state = State::Data;
                    self.emitter.emit_current_tag();
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_error(Error::EofInTag);
                    Ok(ControlToken::Eof)
                }
                Some(x) => {
                    self.emitter.emit_error(Error::UnexpectedSolidusInTag);
                    self.state = State::BeforeAttributeName;
                    self.unread_char(Some(x));
                    Ok(ControlToken::Continue)
                }
            },
            State::BogusComment => match self.read_char()? {
                Some('>') => {
                    self.state = State::Data;
                    self.emitter.emit_current_comment();
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_current_comment();
                    Ok(ControlToken::Eof)
                }
                Some('\0') => {
                    self.emitter.emit_error(Error::UnexpectedNullCharacter);
                    self.emitter.push_comment("\u{fffd}");
                    Ok(ControlToken::Continue)
                }
                Some(x) => {
                    self.emitter.push_comment(ctostr!(x));
                    Ok(ControlToken::Continue)
                }
            },
            State::MarkupDeclarationOpen => match self.read_char()? {
                Some('-') if self.try_read_string("-", true)? => {
                    self.emitter.init_comment();
                    self.state = State::CommentStart;
                    Ok(ControlToken::Continue)
                }
                Some('d' | 'D') if self.try_read_string("octype", false)? => {
                    self.state = State::Doctype;
                    Ok(ControlToken::Continue)
                }
                Some('[') if self.try_read_string("CDATA[", true)? => {
                    // missing: check for adjusted current element: we don't have an element stack
                    // at all
                    //
                    // missing: cdata transition
                    //
                    // let's hope that bogus comment can just sort of skip over cdata
                    self.emitter.emit_error(Error::CdataInHtmlContent);

                    self.emitter.init_comment();
                    self.emitter.push_comment("[CDATA[");
                    self.state = State::BogusComment;
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.emitter.emit_error(Error::IncorrectlyOpenedComment);
                    self.emitter.init_comment();
                    self.state = State::BogusComment;
                    self.unread_char(c);
                    Ok(ControlToken::Continue)
                }
            },
            State::CommentStart => match self.read_char()? {
                Some('-') => {
                    self.state = State::CommentStartDash;
                    Ok(ControlToken::Continue)
                }
                Some('>') => {
                    self.emitter.emit_error(Error::AbruptClosingOfEmptyComment);
                    self.state = State::Data;
                    self.emitter.emit_current_comment();
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.unread_char(c);
                    self.state = State::Comment;
                    Ok(ControlToken::Continue)
                }
            },
            State::CommentStartDash => match self.read_char()? {
                Some('-') => {
                    self.state = State::CommentEnd;
                    Ok(ControlToken::Continue)
                }
                Some('>') => {
                    self.emitter.emit_error(Error::AbruptClosingOfEmptyComment);
                    self.state = State::Data;
                    self.emitter.emit_current_comment();
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_error(Error::EofInComment);
                    self.emitter.emit_current_comment();
                    Ok(ControlToken::Eof)
                }
                c @ Some(_) => {
                    self.emitter.push_comment("-");
                    self.unread_char(c);
                    self.state = State::Comment;
                    Ok(ControlToken::Continue)
                }
            },
            State::Comment => match self.read_char()? {
                Some('<') => {
                    self.emitter.push_comment("<");
                    self.state = State::CommentLessThanSign;
                    Ok(ControlToken::Continue)
                }
                Some('-') => {
                    self.state = State::CommentEndDash;
                    Ok(ControlToken::Continue)
                }
                Some('\0') => {
                    self.emitter.emit_error(Error::UnexpectedNullCharacter);
                    self.emitter.push_comment("\u{fffd}");
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_error(Error::EofInComment);
                    self.emitter.emit_current_comment();
                    Ok(ControlToken::Eof)
                }
                Some(x) => {
                    self.emitter.push_comment(ctostr!(x));
                    Ok(ControlToken::Continue)
                }
            },
            State::CommentLessThanSign => match self.read_char()? {
                Some('!') => {
                    self.emitter.push_comment("!");
                    self.state = State::CommentLessThanSignBang;
                    Ok(ControlToken::Continue)
                }
                Some('<') => {
                    self.emitter.push_comment("<");
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.unread_char(c);
                    self.state = State::Comment;
                    Ok(ControlToken::Continue)
                }
            },
            State::CommentLessThanSignBang => match self.read_char()? {
                Some('-') => {
                    self.state = State::CommentLessThanSignBangDash;
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.unread_char(c);
                    self.state = State::Comment;
                    Ok(ControlToken::Continue)
                }
            },
            State::CommentLessThanSignBangDash => match self.read_char()? {
                Some('-') => {
                    self.state = State::CommentLessThanSignBangDashDash;
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.unread_char(c);
                    self.state = State::CommentEndDash;
                    Ok(ControlToken::Continue)
                }
            },
            State::CommentLessThanSignBangDashDash => match self.read_char()? {
                c @ Some('>') | c @ None => {
                    self.unread_char(c);
                    self.state = State::CommentEnd;
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.emitter.emit_error(Error::NestedComment);
                    self.unread_char(c);
                    self.state = State::CommentEnd;
                    Ok(ControlToken::Continue)
                }
            },
            State::CommentEndDash => match self.read_char()? {
                Some('-') => {
                    self.state = State::CommentEnd;
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_error(Error::EofInComment);
                    self.emitter.emit_current_comment();
                    Ok(ControlToken::Eof)
                }
                c => {
                    self.emitter.push_comment("-");
                    self.unread_char(c);
                    self.state = State::Comment;
                    Ok(ControlToken::Continue)
                }
            },
            State::CommentEnd => match self.read_char()? {
                Some('>') => {
                    self.state = State::Data;
                    self.emitter.emit_current_comment();
                    Ok(ControlToken::Continue)
                }
                Some('!') => {
                    self.state = State::CommentEndBang;
                    Ok(ControlToken::Continue)
                }
                Some('-') => {
                    self.emitter.push_comment("-");
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_error(Error::EofInComment);
                    self.emitter.emit_current_comment();
                    Ok(ControlToken::Eof)
                }
                c @ Some(_) => {
                    self.emitter.push_comment("-");
                    self.emitter.push_comment("-");
                    self.unread_char(c);
                    self.state = State::Comment;
                    Ok(ControlToken::Continue)
                }
            },
            State::CommentEndBang => match self.read_char()? {
                Some('-') => {
                    self.emitter.push_comment("-");
                    self.emitter.push_comment("-");
                    self.emitter.push_comment("!");
                    self.state = State::CommentEndDash;
                    Ok(ControlToken::Continue)
                }
                Some('>') => {
                    self.emitter.emit_error(Error::IncorrectlyClosedComment);
                    self.state = State::Data;
                    self.emitter.emit_current_comment();
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_error(Error::EofInComment);
                    self.emitter.emit_current_comment();
                    Ok(ControlToken::Eof)
                }
                c @ Some(_) => {
                    self.emitter.push_comment("-");
                    self.emitter.push_comment("-");
                    self.emitter.push_comment("!");
                    self.state = State::Comment;
                    self.unread_char(c);
                    Ok(ControlToken::Continue)
                }
            },
            State::Doctype => match self.read_char()? {
                Some(whitespace_pat!()) => {
                    self.state = State::BeforeDoctypeName;
                    Ok(ControlToken::Continue)
                }
                c @ Some('>') => {
                    self.unread_char(c);
                    self.state = State::BeforeDoctypeName;
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_error(Error::EofInDoctype);
                    self.emitter.init_doctype();
                    self.emitter.set_force_quirks();
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Eof)
                }
                c @ Some(_) => {
                    self.emitter
                        .emit_error(Error::MissingWhitespaceBeforeDoctypeName);
                    self.unread_char(c);
                    self.state = State::BeforeDoctypeName;
                    Ok(ControlToken::Continue)
                }
            },
            State::BeforeDoctypeName => match self.read_char()? {
                Some(whitespace_pat!()) => Ok(ControlToken::Continue),
                Some('\0') => {
                    self.emitter.emit_error(Error::UnexpectedNullCharacter);
                    self.emitter.init_doctype();
                    self.emitter.push_doctype_name("\u{fffd}");
                    self.state = State::DoctypeName;
                    Ok(ControlToken::Continue)
                }
                Some('>') => {
                    self.emitter.emit_error(Error::MissingDoctypeName);
                    self.emitter.init_doctype();
                    self.emitter.set_force_quirks();
                    self.state = State::Data;
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_error(Error::EofInDoctype);
                    self.emitter.init_doctype();
                    self.emitter.set_force_quirks();
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Eof)
                }
                Some(x) => {
                    self.emitter.init_doctype();
                    self.emitter
                        .push_doctype_name(ctostr!(x.to_ascii_lowercase()));
                    self.state = State::DoctypeName;
                    Ok(ControlToken::Continue)
                }
            },
            State::DoctypeName => match self.read_char()? {
                Some(whitespace_pat!()) => {
                    self.state = State::AfterDoctypeName;
                    Ok(ControlToken::Continue)
                }
                Some('>') => {
                    self.state = State::Data;
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Continue)
                }
                Some('\0') => {
                    self.emitter.emit_error(Error::UnexpectedNullCharacter);
                    self.emitter.push_doctype_name("\u{fffd}");
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_error(Error::EofInDoctype);
                    self.emitter.set_force_quirks();
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Eof)
                }
                Some(x) => {
                    self.emitter
                        .push_doctype_name(ctostr!(x.to_ascii_lowercase()));
                    Ok(ControlToken::Continue)
                }
            },
            State::AfterDoctypeName => match self.read_char()? {
                Some(whitespace_pat!()) => Ok(ControlToken::Continue),
                Some('>') => {
                    self.state = State::Data;
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_error(Error::EofInDoctype);
                    self.emitter.set_force_quirks();
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Eof)
                }
                Some('p' | 'P') if self.try_read_string("ublic", false)? => {
                    self.state = State::AfterDoctypePublicKeyword;
                    Ok(ControlToken::Continue)
                }
                Some('s' | 'S') if self.try_read_string("ystem", false)? => {
                    self.state = State::AfterDoctypeSystemKeyword;
                    Ok(ControlToken::Continue)
                }
                c @ Some(_) => {
                    self.emitter
                        .emit_error(Error::InvalidCharacterSequenceAfterDoctypeName);
                    self.emitter.set_force_quirks();
                    self.unread_char(c);
                    self.state = State::BogusDoctype;
                    Ok(ControlToken::Continue)
                }
            },
            State::AfterDoctypePublicKeyword => match self.read_char()? {
                Some(whitespace_pat!()) => {
                    self.state = State::BeforeDoctypePublicIdentifier;
                    Ok(ControlToken::Continue)
                }
                Some('"') => {
                    self.emitter
                        .emit_error(Error::MissingWhitespaceAfterDoctypePublicKeyword);
                    self.emitter.set_doctype_public_identifier("");
                    self.state = State::DoctypePublicIdentifierDoubleQuoted;
                    Ok(ControlToken::Continue)
                }
                Some('\'') => {
                    self.emitter
                        .emit_error(Error::MissingWhitespaceAfterDoctypePublicKeyword);
                    self.emitter.set_doctype_public_identifier("");
                    self.state = State::DoctypePublicIdentifierSingleQuoted;
                    Ok(ControlToken::Continue)
                }
                Some('>') => {
                    self.emitter
                        .emit_error(Error::MissingDoctypePublicIdentifier);
                    self.emitter.set_force_quirks();
                    self.state = State::Data;
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_error(Error::EofInDoctype);
                    self.emitter.set_force_quirks();
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Eof)
                }
                c @ Some(_) => {
                    self.emitter
                        .emit_error(Error::MissingQuoteBeforeDoctypePublicIdentifier);
                    self.emitter.set_force_quirks();
                    self.unread_char(c);
                    self.state = State::BogusDoctype;
                    Ok(ControlToken::Continue)
                }
            },
            State::BeforeDoctypePublicIdentifier => match self.read_char()? {
                Some(whitespace_pat!()) => Ok(ControlToken::Continue),
                Some('"') => {
                    self.emitter.set_doctype_public_identifier("");
                    self.state = State::DoctypePublicIdentifierDoubleQuoted;
                    Ok(ControlToken::Continue)
                }
                Some('\'') => {
                    self.emitter.set_doctype_public_identifier("");
                    self.state = State::DoctypePublicIdentifierSingleQuoted;
                    Ok(ControlToken::Continue)
                }
                Some('>') => {
                    self.emitter
                        .emit_error(Error::MissingDoctypePublicIdentifier);
                    self.emitter.set_force_quirks();
                    self.state = State::Data;
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_error(Error::EofInDoctype);
                    self.emitter.set_force_quirks();
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Eof)
                }
                c @ Some(_) => {
                    self.emitter
                        .emit_error(Error::MissingQuoteBeforeDoctypePublicIdentifier);
                    self.emitter.set_force_quirks();
                    self.unread_char(c);
                    self.state = State::BogusDoctype;
                    Ok(ControlToken::Continue)
                }
            },
            State::DoctypePublicIdentifierDoubleQuoted => match self.read_char()? {
                Some('"') => {
                    self.state = State::AfterDoctypePublicIdentifier;
                    Ok(ControlToken::Continue)
                }
                Some('\0') => {
                    self.emitter.emit_error(Error::UnexpectedNullCharacter);
                    self.emitter.push_doctype_public_identifier("\u{fffd}");
                    Ok(ControlToken::Continue)
                }
                Some('>') => {
                    self.emitter
                        .emit_error(Error::AbruptDoctypePublicIdentifier);
                    self.emitter.set_force_quirks();
                    self.state = State::Data;
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_error(Error::EofInDoctype);
                    self.emitter.set_force_quirks();
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Eof)
                }
                Some(x) => {
                    self.emitter.push_doctype_public_identifier(ctostr!(x));
                    Ok(ControlToken::Continue)
                }
            },
            State::DoctypePublicIdentifierSingleQuoted => match self.read_char()? {
                Some('\'') => {
                    self.state = State::AfterDoctypePublicIdentifier;
                    Ok(ControlToken::Continue)
                }
                Some('\0') => {
                    self.emitter.emit_error(Error::UnexpectedNullCharacter);
                    self.emitter.push_doctype_public_identifier("\u{fffd}");
                    Ok(ControlToken::Continue)
                }
                Some('>') => {
                    self.emitter
                        .emit_error(Error::AbruptDoctypePublicIdentifier);
                    self.emitter.set_force_quirks();
                    self.state = State::Data;
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_error(Error::EofInDoctype);
                    self.emitter.set_force_quirks();
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Eof)
                }
                Some(x) => {
                    self.emitter.push_doctype_public_identifier(ctostr!(x));
                    Ok(ControlToken::Continue)
                }
            },
            State::AfterDoctypePublicIdentifier => match self.read_char()? {
                Some(whitespace_pat!()) => {
                    self.state = State::BetweenDoctypePublicAndSystemIdentifiers;
                    Ok(ControlToken::Continue)
                }
                Some('>') => {
                    self.state = State::Data;
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Continue)
                }
                Some('"') => {
                    self.emitter.emit_error(
                        Error::MissingWhitespaceBetweenDoctypePublicAndSystemIdentifiers,
                    );
                    self.emitter.set_doctype_system_identifier("");
                    self.state = State::DoctypeSystemIdentifierDoubleQuoted;
                    Ok(ControlToken::Continue)
                }
                Some('\'') => {
                    self.emitter.emit_error(
                        Error::MissingWhitespaceBetweenDoctypePublicAndSystemIdentifiers,
                    );
                    self.emitter.set_doctype_system_identifier("");
                    self.state = State::DoctypeSystemIdentifierSingleQuoted;
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_error(Error::EofInDoctype);
                    self.emitter.set_force_quirks();
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Eof)
                }
                c @ Some(_) => {
                    self.emitter
                        .emit_error(Error::MissingQuoteBeforeDoctypeSystemIdentifier);
                    self.emitter.set_force_quirks();
                    self.unread_char(c);
                    self.state = State::BogusDoctype;
                    Ok(ControlToken::Continue)
                }
            },
            State::BetweenDoctypePublicAndSystemIdentifiers => match self.read_char()? {
                Some(whitespace_pat!()) => Ok(ControlToken::Continue),
                Some('>') => {
                    self.state = State::Data;
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Continue)
                }
                Some('"') => {
                    self.emitter.set_doctype_system_identifier("");
                    self.state = State::DoctypeSystemIdentifierDoubleQuoted;
                    Ok(ControlToken::Continue)
                }
                Some('\'') => {
                    self.emitter.set_doctype_system_identifier("");
                    self.state = State::DoctypeSystemIdentifierSingleQuoted;
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_error(Error::EofInDoctype);
                    self.emitter.set_force_quirks();
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Eof)
                }
                c @ Some(_) => {
                    self.emitter
                        .emit_error(Error::MissingQuoteBeforeDoctypeSystemIdentifier);
                    self.emitter.set_force_quirks();
                    self.state = State::BogusDoctype;
                    self.unread_char(c);
                    Ok(ControlToken::Continue)
                }
            },
            State::AfterDoctypeSystemKeyword => match self.read_char()? {
                Some(whitespace_pat!()) => {
                    self.state = State::BeforeDoctypeSystemIdentifier;
                    Ok(ControlToken::Continue)
                }
                Some('"') => {
                    self.emitter
                        .emit_error(Error::MissingWhitespaceAfterDoctypeSystemKeyword);
                    self.emitter.set_doctype_system_identifier("");
                    self.state = State::DoctypeSystemIdentifierDoubleQuoted;
                    Ok(ControlToken::Continue)
                }
                Some('\'') => {
                    self.emitter
                        .emit_error(Error::MissingWhitespaceAfterDoctypeSystemKeyword);
                    self.emitter.set_doctype_system_identifier("");
                    self.state = State::DoctypeSystemIdentifierSingleQuoted;
                    Ok(ControlToken::Continue)
                }
                Some('>') => {
                    self.emitter
                        .emit_error(Error::MissingDoctypeSystemIdentifier);
                    self.emitter.set_force_quirks();
                    self.state = State::Data;
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_error(Error::EofInDoctype);
                    self.emitter.set_force_quirks();
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Eof)
                }
                c @ Some(_) => {
                    self.emitter
                        .emit_error(Error::MissingQuoteBeforeDoctypeSystemIdentifier);
                    self.emitter.set_force_quirks();
                    self.state = State::BogusDoctype;
                    self.unread_char(c);
                    Ok(ControlToken::Continue)
                }
            },
            State::BeforeDoctypeSystemIdentifier => match self.read_char()? {
                Some(whitespace_pat!()) => Ok(ControlToken::Continue),
                Some('"') => {
                    self.emitter.set_doctype_system_identifier("");
                    self.state = State::DoctypeSystemIdentifierDoubleQuoted;
                    Ok(ControlToken::Continue)
                }
                Some('\'') => {
                    self.emitter.set_doctype_system_identifier("");
                    self.state = State::DoctypeSystemIdentifierSingleQuoted;
                    Ok(ControlToken::Continue)
                }
                Some('>') => {
                    self.emitter
                        .emit_error(Error::MissingDoctypeSystemIdentifier);
                    self.emitter.set_force_quirks();
                    self.state = State::Data;
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_error(Error::EofInDoctype);
                    self.emitter.set_force_quirks();
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Eof)
                }
                c @ Some(_) => {
                    self.emitter
                        .emit_error(Error::MissingQuoteBeforeDoctypeSystemIdentifier);
                    self.emitter.set_force_quirks();
                    self.state = State::BogusDoctype;
                    self.unread_char(c);
                    Ok(ControlToken::Continue)
                }
            },
            State::DoctypeSystemIdentifierDoubleQuoted => match self.read_char()? {
                Some('"') => {
                    self.state = State::AfterDoctypeSystemIdentifier;
                    Ok(ControlToken::Continue)
                }
                Some('\0') => {
                    self.emitter.emit_error(Error::UnexpectedNullCharacter);
                    self.emitter.push_doctype_system_identifier("\u{fffd}");
                    Ok(ControlToken::Continue)
                }
                Some('>') => {
                    self.emitter
                        .emit_error(Error::AbruptDoctypeSystemIdentifier);
                    self.emitter.set_force_quirks();
                    self.state = State::Data;
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_error(Error::EofInDoctype);
                    self.emitter.set_force_quirks();
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Eof)
                }
                Some(x) => {
                    self.emitter.push_doctype_system_identifier(ctostr!(x));
                    Ok(ControlToken::Continue)
                }
            },
            State::DoctypeSystemIdentifierSingleQuoted => match self.read_char()? {
                Some('\'') => {
                    self.state = State::AfterDoctypeSystemIdentifier;
                    Ok(ControlToken::Continue)
                }
                Some('\0') => {
                    self.emitter.emit_error(Error::UnexpectedNullCharacter);
                    self.emitter.push_doctype_system_identifier("\u{fffd}");
                    Ok(ControlToken::Continue)
                }
                Some('>') => {
                    self.emitter
                        .emit_error(Error::AbruptDoctypeSystemIdentifier);
                    self.emitter.set_force_quirks();
                    self.state = State::Data;
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_error(Error::EofInDoctype);
                    self.emitter.set_force_quirks();
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Eof)
                }
                Some(x) => {
                    self.emitter.push_doctype_system_identifier(ctostr!(x));
                    Ok(ControlToken::Continue)
                }
            },
            State::AfterDoctypeSystemIdentifier => match self.read_char()? {
                Some(whitespace_pat!()) => Ok(ControlToken::Continue),
                Some('>') => {
                    self.state = State::Data;
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_error(Error::EofInDoctype);
                    self.emitter.set_force_quirks();
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Eof)
                }
                c @ Some(_) => {
                    self.emitter
                        .emit_error(Error::UnexpectedCharacterAfterDoctypeSystemIdentifier);
                    self.unread_char(c);
                    self.state = State::BogusDoctype;
                    Ok(ControlToken::Continue)
                }
            },
            State::BogusDoctype => match self.read_char()? {
                Some('>') => {
                    self.state = State::Data;
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Continue)
                }
                Some('\0') => {
                    self.emitter.emit_error(Error::UnexpectedNullCharacter);
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_current_doctype();
                    Ok(ControlToken::Eof)
                }
                Some(_) => Ok(ControlToken::Continue),
            },
            State::CdataSection => match self.read_char()? {
                Some(']') => {
                    self.state = State::CdataSectionBracket;
                    Ok(ControlToken::Continue)
                }
                None => {
                    self.emitter.emit_error(Error::EofInCdata);
                    Ok(ControlToken::Eof)
                }
                Some(x) => {
                    self.emitter.emit_string(ctostr!(x));
                    Ok(ControlToken::Continue)
                }
            },
            State::CdataSectionBracket => match self.read_char()? {
                Some(']') => {
                    self.state = State::CdataSectionEnd;
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.emitter.emit_string("]");
                    self.state = State::CdataSection;
                    self.unread_char(c);
                    Ok(ControlToken::Continue)
                }
            },
            State::CdataSectionEnd => match self.read_char()? {
                Some(']') => {
                    self.emitter.emit_string("]");
                    Ok(ControlToken::Continue)
                }
                Some('>') => {
                    self.state = State::Data;
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.emitter.emit_string("]]");
                    self.unread_char(c);
                    self.state = State::CdataSection;
                    Ok(ControlToken::Continue)
                }
            },
            State::CharacterReference => {
                self.temporary_buffer.clear();
                self.temporary_buffer.push('&');
                match self.read_char()? {
                    Some(x) if x.is_ascii_alphanumeric() => {
                        self.unread_char(Some(x));
                        self.state = State::NamedCharacterReference;
                        Ok(ControlToken::Continue)
                    }
                    Some('#') => {
                        self.temporary_buffer.push('#');
                        self.state = State::NumericCharacterReference;
                        Ok(ControlToken::Continue)
                    }
                    c => {
                        self.flush_code_points_consumed_as_character_reference();
                        self.state = self.return_state.take().unwrap();
                        self.unread_char(c);
                        Ok(ControlToken::Continue)
                    }
                }
            }
            State::NamedCharacterReference => {
                let c = self.read_char()?;

                let char_ref = match c {
                    Some(x) => entities::try_read_character_reference(x, |x| {
                        self.try_read_string(x, true)
                    })?
                    .map(|char_ref| (x, char_ref)),

                    None => None,
                };

                if let Some((x, char_ref)) = char_ref {
                    self.temporary_buffer.push(x);
                    self.temporary_buffer.push_str(char_ref.name);
                    let char_ref_name_last_character = char_ref.name.chars().last();
                    let next_character = self.next_input_character()?;
                    if self.is_consumed_as_part_of_an_attribute()
                        && char_ref_name_last_character != Some(';')
                        && matches!(next_character, Some(x) if x == '=' || x.is_ascii_alphanumeric())
                    {
                        self.flush_code_points_consumed_as_character_reference();
                        self.state = self.return_state.take().unwrap();
                        Ok(ControlToken::Continue)
                    } else {
                        if char_ref_name_last_character != Some(';') {
                            self.emitter
                                .emit_error(Error::MissingSemicolonAfterCharacterReference);
                        }

                        self.temporary_buffer.clear();
                        self.temporary_buffer.push_str(char_ref.characters);
                        self.flush_code_points_consumed_as_character_reference();
                        self.state = self.return_state.take().unwrap();
                        Ok(ControlToken::Continue)
                    }
                } else {
                    self.unread_char(c);
                    self.flush_code_points_consumed_as_character_reference();
                    self.state = State::AmbiguousAmpersand;
                    Ok(ControlToken::Continue)
                }
            }
            State::AmbiguousAmpersand => match self.read_char()? {
                Some(x) if x.is_ascii_alphanumeric() => {
                    if self.is_consumed_as_part_of_an_attribute() {
                        self.emitter.push_attribute_value(ctostr!(x));
                    } else {
                        self.emitter.emit_string(ctostr!(x));
                    }

                    Ok(ControlToken::Continue)
                }
                c @ Some(';') => {
                    self.emitter
                        .emit_error(Error::UnknownNamedCharacterReference);
                    self.unread_char(c);
                    self.state = self.return_state.take().unwrap();
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.unread_char(c);
                    self.state = self.return_state.take().unwrap();
                    Ok(ControlToken::Continue)
                }
            },
            State::NumericCharacterReference => {
                self.character_reference_code = 0;
                match self.read_char()? {
                    Some(x @ 'x' | x @ 'X') => {
                        self.temporary_buffer.push(x);
                        self.state = State::HexadecimalCharacterReferenceStart;
                        Ok(ControlToken::Continue)
                    }
                    c => {
                        self.unread_char(c);
                        self.state = State::DecimalCharacterReferenceStart;
                        Ok(ControlToken::Continue)
                    }
                }
            }
            State::HexadecimalCharacterReferenceStart => match self.read_char()? {
                c @ Some('0'..='9' | 'A'..='F' | 'a'..='f') => {
                    self.unread_char(c);
                    self.state = State::HexadecimalCharacterReference;
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.emitter
                        .emit_error(Error::AbsenceOfDigitsInNumericCharacterReference);
                    self.flush_code_points_consumed_as_character_reference();
                    self.unread_char(c);
                    self.state = self.return_state.take().unwrap();
                    Ok(ControlToken::Continue)
                }
            },
            State::DecimalCharacterReferenceStart => match self.read_char()? {
                Some(x @ ascii_digit_pat!()) => {
                    self.unread_char(Some(x));
                    self.state = State::DecimalCharacterReference;
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.emitter
                        .emit_error(Error::AbsenceOfDigitsInNumericCharacterReference);
                    self.flush_code_points_consumed_as_character_reference();
                    self.unread_char(c);
                    self.state = self.return_state.take().unwrap();
                    Ok(ControlToken::Continue)
                }
            },
            State::HexadecimalCharacterReference => match self.read_char()? {
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
                    self.state = State::NumericCharacterReferenceEnd;
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.emitter
                        .emit_error(Error::MissingSemicolonAfterCharacterReference);
                    self.unread_char(c);
                    self.state = State::NumericCharacterReferenceEnd;
                    Ok(ControlToken::Continue)
                }
            },
            State::DecimalCharacterReference => match self.read_char()? {
                Some(x @ ascii_digit_pat!()) => {
                    mutate_character_reference!(*10 + x - 0x0030);
                    Ok(ControlToken::Continue)
                }
                Some(';') => {
                    self.state = State::NumericCharacterReferenceEnd;
                    Ok(ControlToken::Continue)
                }
                c => {
                    self.emitter
                        .emit_error(Error::MissingSemicolonAfterCharacterReference);
                    self.unread_char(c);
                    self.state = State::NumericCharacterReferenceEnd;
                    Ok(ControlToken::Continue)
                }
            },
            State::NumericCharacterReferenceEnd => {
                match self.character_reference_code {
                    0x00 => {
                        self.emitter.emit_error(Error::NullCharacterReference);
                        self.character_reference_code = 0xfffd;
                    }
                    0x110000.. => {
                        self.emitter
                            .emit_error(Error::CharacterReferenceOutsideUnicodeRange);
                        self.character_reference_code = 0xfffd;
                    }
                    surrogate_pat!() => {
                        self.emitter.emit_error(Error::SurrogateCharacterReference);
                        self.character_reference_code = 0xfffd;
                    }
                    // noncharacter
                    noncharacter_pat!() => {
                        self.emitter
                            .emit_error(Error::NoncharacterCharacterReference);
                    }
                    // 0x000d, or a control that is not whitespace
                    x @ 0x000d | x @ control_pat!()
                        if !matches!(x, 0x0009 | 0x000a | 0x000c | 0x0020) =>
                    {
                        self.emitter.emit_error(Error::ControlCharacterReference);
                        self.character_reference_code = match x {
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
                            _ => self.character_reference_code,
                        };
                    }
                    _ => (),
                }

                self.temporary_buffer.clear();
                self.temporary_buffer
                    .push(std::char::from_u32(self.character_reference_code).unwrap());
                self.flush_code_points_consumed_as_character_reference();
                self.state = self.return_state.take().unwrap();
                Ok(ControlToken::Continue)
            }
        }
    }
}

impl<R: Reader, E: Emitter> Iterator for Tokenizer<R, E> {
    type Item = Result<E::Token, R::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(token) = self.emitter.pop_token() {
                break Some(Ok(token));
            } else if !self.eof {
                match self.consume() {
                    Ok(ControlToken::Continue) => (),
                    Ok(ControlToken::Eof) => {
                        self.eof = true;
                        self.emitter.emit_eof();
                    }
                    Err(e) => break Some(Err(e)),
                }
            } else {
                break None;
            }
        }
    }
}

/// A kind of tokenizer that directly yields tokens when used as an iterator, so `Token` instead of
/// `Result<Token, _>`.
///
/// This is the return value of [`Tokenizer::infallible`].
pub struct InfallibleTokenizer<R: Reader<Error = Never>, E: Emitter>(Tokenizer<R, E>);

impl<R: Reader<Error = Never>, E: Emitter> Tokenizer<R, E> {
    /// Statically assert that this iterator is infallible.
    ///
    /// Call this to get rid of error handling when parsing HTML from strings.
    pub fn infallible(self) -> InfallibleTokenizer<R, E> {
        InfallibleTokenizer(self)
    }
}

impl<R: Reader<Error = Never>, E: Emitter> Iterator for InfallibleTokenizer<R, E> {
    type Item = E::Token;

    fn next(&mut self) -> Option<Self::Item> {
        match self.0.next()? {
            Ok(token) => Some(token),
            Err(e) => match e {},
        }
    }
}
