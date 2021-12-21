use crate::machine;
use crate::machine_helper::MachineHelper;
use crate::read_helper::ReadHelper;
use crate::utils::{ControlToken, State};
use crate::{DefaultEmitter, Emitter, Never, Readable, Reader};

/// A HTML tokenizer. See crate-level docs for basic usage.
pub struct Tokenizer<R: Reader, E: Emitter = DefaultEmitter> {
    eof: bool,
    pub(crate) emitter: E,
    pub(crate) reader: ReadHelper<R>,
    pub(crate) machine_helper: MachineHelper,
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
            emitter,
            reader: ReadHelper::new(input.to_reader()),
            machine_helper: MachineHelper::default(),
        }
    }

    /// Test-internal function to override internal state.
    ///
    /// Only available with the `integration-tests` feature which is not public API.
    #[cfg(feature = "integration-tests")]
    pub fn set_state(&mut self, state: State) {
        self.machine_helper.state = state;
    }

    /// Set the statemachine to start/continue in [plaintext
    /// state](https://html.spec.whatwg.org/#plaintext-state).
    ///
    /// This tokenizer never gets into that state naturally.
    pub fn set_plaintext_state(&mut self) {
        self.machine_helper.state = State::PlainText;
    }

    /// Test-internal function to override internal state.
    ///
    /// Only available with the `integration-tests` feature which is not public API.
    #[cfg(feature = "integration-tests")]
    pub fn set_last_start_tag(&mut self, last_start_tag: Option<&str>) {
        self.emitter.set_last_start_tag(last_start_tag);
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
                    Ok(ControlToken::Reconsume(c, state)) => {
                        self.machine_helper.switch_to(state);
                        self.reader.unread_char(c);
                    }
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
