use crate::machine;
use crate::utils::{control_pat, noncharacter_pat, surrogate_pat, ControlToken, State};
use crate::{DefaultEmitter, Emitter, Error, Never, Readable, Reader};

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
    pub(crate) state: State,
    pub(crate) emitter: E,
    pub(crate) temporary_buffer: String,
    reader: R,
    to_reconsume: Stack2<Option<char>>,
    pub(crate) character_reference_code: u32,
    pub(crate) return_state: Option<State>,
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

    /// Test-internal function to override internal state.
    ///
    /// Only available with the `integration-tests` feature which is not public API.
    #[cfg(feature = "integration-tests")]
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

    /// Test-internal function to override internal state.
    ///
    /// Only available with the `integration-tests` feature which is not public API.
    #[cfg(feature = "integration-tests")]
    pub fn set_last_start_tag(&mut self, last_start_tag: Option<&str>) {
        self.emitter.set_last_start_tag(last_start_tag);
    }

    #[inline]
    pub(crate) fn unread_char(&mut self, c: Option<char>) {
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

    pub(crate) fn read_char(&mut self) -> Result<Option<char>, R::Error> {
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
    pub(crate) fn try_read_string(
        &mut self,
        s: &str,
        case_sensitive: bool,
    ) -> Result<bool, R::Error> {
        debug_assert!(!s.is_empty());
        debug_assert!(self.to_reconsume.is_empty());
        self.reader.try_read_string(s, case_sensitive)
    }

    pub(crate) fn is_consumed_as_part_of_an_attribute(&self) -> bool {
        matches!(
            self.return_state,
            Some(
                State::AttributeValueDoubleQuoted
                    | State::AttributeValueSingleQuoted
                    | State::AttributeValueUnquoted
            )
        )
    }

    pub(crate) fn flush_code_points_consumed_as_character_reference(&mut self) {
        if self.is_consumed_as_part_of_an_attribute() {
            self.emitter.push_attribute_value(&self.temporary_buffer);
            self.temporary_buffer.clear();
        } else {
            self.flush_buffer_characters();
        }
    }

    pub(crate) fn next_input_character(&mut self) -> Result<Option<char>, R::Error> {
        let rv = self.read_char()?;
        self.unread_char(rv);
        Ok(rv)
    }

    pub(crate) fn flush_buffer_characters(&mut self) {
        self.emitter.emit_string(&self.temporary_buffer);
        self.temporary_buffer.clear();
    }
}

impl<R: Reader, E: Emitter> Iterator for Tokenizer<R, E> {
    type Item = Result<E::Token, R::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(token) = self.emitter.pop_token() {
                break Some(Ok(token));
            } else if !self.eof {
                match machine::consume(self) {
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
