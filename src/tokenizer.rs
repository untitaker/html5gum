use std::convert::Infallible;

use crate::char_validator::CharValidator;
use crate::machine_helper::{ControlToken, MachineHelper};
use crate::read_helper::ReadHelper;
use crate::State;
use crate::{DefaultEmitter, Emitter, Readable, Reader};

/// A HTML tokenizer. See crate-level docs for basic usage.
#[derive(Debug)]
pub struct Tokenizer<R: Reader, E: Emitter = DefaultEmitter> {
    eof: bool,
    pub(crate) validator: CharValidator,
    pub(crate) emitter: E,
    pub(crate) reader: ReadHelper<R>,
    pub(crate) machine_helper: MachineHelper<R, E>,
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
            validator: CharValidator::default(),
            emitter,
            reader: ReadHelper::new(input.to_reader()),
            machine_helper: MachineHelper::default(),
        }
    }

    /// Override internal state. Necessary for parsing partial documents ("fragment parsing")
    pub fn set_state(&mut self, state: State) {
        self.machine_helper.state = state.into();
    }

    /// Test-internal function to override internal state.
    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn set_last_start_tag(&mut self, last_start_tag: Option<&str>) {
        self.emitter
            .set_last_start_tag(last_start_tag.map(str::as_bytes));
    }
}

impl<R: Reader, E: Emitter<Token = Infallible>> Tokenizer<R, E> {
    /// Some emitters don't ever produce any tokens and instead have other side effects. In those
    /// cases, you will find yourself writing code like this to handle errors:
    ///
    /// ```
    /// use std::convert::Infallible;
    ///
    /// use html5gum::Tokenizer;
    /// use html5gum::emitters::callback::{CallbackEvent, CallbackEmitter};
    ///
    /// let emitter = CallbackEmitter::new(move |event: CallbackEvent<'_>| -> Option<Infallible> {
    ///     if let CallbackEvent::String { value } = event {
    ///         println!("{}", String::from_utf8_lossy(value));
    ///     }
    ///
    ///     // We may choose to return any Option<T> (such as errors, or our own tokens), but since
    ///     // we do all the real work in the callback itself, we choose to use Option<Infallible>.
    ///     None
    /// });
    ///
    /// let tokenizer = Tokenizer::new_with_emitter("hello <div><div><div> world!", emitter);
    ///
    /// // this is a bit silly
    /// // for _ in tokenizer {
    /// //     result.unwrap();
    /// // }
    ///
    /// // much better:
    /// tokenizer.finish();
    /// ```
    pub fn finish(self) -> Result<(), R::Error> {
        for result in self {
            result?;
        }

        Ok(())
    }
}

impl<R: Reader, E: Emitter> Iterator for Tokenizer<R, E> {
    type Item = Result<E::Token, R::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(token) = self.emitter.pop_token() {
                break Some(Ok(token));
            } else if !self.eof {
                match (self.machine_helper.state.function)(self) {
                    Ok(ControlToken::Continue) => (),
                    Ok(ControlToken::SwitchTo(next_state)) => {
                        self.machine_helper.switch_to(next_state);
                    }
                    Ok(ControlToken::Eof) => {
                        self.validator.flush_character_error(&mut self.emitter);
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
#[derive(Debug)]
pub struct InfallibleTokenizer<R: Reader<Error = Infallible>, E: Emitter>(Tokenizer<R, E>);

impl<R: Reader<Error = Infallible>, E: Emitter> Tokenizer<R, E> {
    /// Statically assert that this iterator is infallible.
    ///
    /// Call this to get rid of error handling when parsing HTML from strings.
    pub fn infallible(self) -> InfallibleTokenizer<R, E> {
        InfallibleTokenizer(self)
    }
}

impl<R: Reader<Error = Infallible>, E: Emitter> Iterator for InfallibleTokenizer<R, E> {
    type Item = E::Token;

    fn next(&mut self) -> Option<Self::Item> {
        match self.0.next()? {
            Ok(token) => Some(token),
            Err(e) => match e {},
        }
    }
}
