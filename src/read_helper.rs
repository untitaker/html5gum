use crate::char_validator::CharValidator;
use crate::Emitter;
use crate::Reader;

#[derive(Debug)]
pub(crate) struct ReadHelper<R: Reader> {
    pub(crate) reader: R,
    last_character_was_cr: bool,
}

impl<R: Reader> ReadHelper<R> {
    pub(crate) fn new(reader: R) -> Self {
        ReadHelper {
            reader,
            last_character_was_cr: false,
        }
    }

    #[inline(always)]
    pub(crate) fn read_byte<E: Emitter>(
        &mut self,
        char_validator: &mut CharValidator,
        emitter: &mut E,
    ) -> Result<Option<u8>, R::Error> {
        let c = self.reader.read_byte();
        self.preprocess_byte(c, char_validator, emitter)
    }

    /// Read the next byte as it appears in the input, without preprocessing and validating it.
    ///
    /// This allows to look at the next byte before deciding whether it is consumed. Unlike with
    /// reading and later reconsuming it, this does not change the order in which errors are
    /// emitted.
    ///
    /// The byte has to be passed to `consume_raw_byte` before reading anything else.
    #[inline(always)]
    pub(crate) fn read_raw_byte(&mut self) -> Result<Option<u8>, R::Error> {
        self.reader.read_byte()
    }

    /// Preprocess and validate a byte returned by `read_raw_byte`, exactly like `read_byte` would
    /// have done.
    #[inline]
    pub(crate) fn consume_raw_byte<E: Emitter>(
        &mut self,
        c: Option<u8>,
        char_validator: &mut CharValidator,
        emitter: &mut E,
    ) -> Result<Option<u8>, R::Error> {
        self.preprocess_byte(Ok(c), char_validator, emitter)
    }

    /// Newline normalization and validation of a byte that was taken from the reader.
    #[inline(always)]
    fn preprocess_byte<E: Emitter>(
        &mut self,
        mut c: Result<Option<u8>, R::Error>,
        char_validator: &mut CharValidator,
        emitter: &mut E,
    ) -> Result<Option<u8>, R::Error> {
        if self.last_character_was_cr && matches!(c, Ok(Some(b'\n'))) {
            // TODO: we are scattering move_position in the read
            // path across many places: read helper, tokenizer.rs,
            // ... consolidate!
            emitter.move_position(1);
            c = self.reader.read_byte();
        }

        if matches!(c, Ok(Some(b'\r'))) {
            self.last_character_was_cr = true;
            c = Ok(Some(b'\n'));
        } else {
            self.last_character_was_cr = false;
        }

        if let Ok(Some(x)) = c {
            char_validator.validate_byte(emitter, x);
        }

        c
    }

    #[inline(always)]
    pub(crate) fn try_read_string(
        &mut self,
        char_validator: &mut CharValidator,
        s: &str,
        case_sensitive: bool,
    ) -> Result<bool, R::Error> {
        debug_assert!(!s.is_empty());
        debug_assert!(!s.contains('\r'));

        if self.reader.try_read_string(s.as_bytes(), case_sensitive)? {
            self.last_character_was_cr = false;
            char_validator.reset();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    #[inline(always)]
    pub(crate) fn read_until<'b, E: Emitter>(
        &'b mut self,
        needle: &[u8],
        char_validator: &mut CharValidator,
        emitter: &mut E,
        char_buf: &'b mut [u8; 4],
    ) -> Result<Option<&'b [u8]>, R::Error> {
        const MAX_NEEDLE_LEN: usize = 13;

        let mut needle2 = [b'\0'; MAX_NEEDLE_LEN];
        // Assert that we will have space for adding \r
        // If not, just bump MAX_NEEDLE_LEN
        debug_assert!(needle.len() < needle2.len());
        needle2[..needle.len()].copy_from_slice(needle);
        needle2[needle.len()] = b'\r';
        let needle2_slice = &needle2[..=needle.len()];

        let read = self.reader.read_until(needle2_slice, char_buf)?;
        match read {
            Some(b"\r") => {
                self.last_character_was_cr = true;
                char_validator.validate_byte(emitter, b'\n');
                Ok(Some(b"\n"))
            }
            Some(mut xs) => {
                char_validator.validate_bytes(emitter, xs);

                // Only store when the flag is set: this runs for every chunk.
                if self.last_character_was_cr {
                    if xs.starts_with(b"\n") {
                        // TODO: we are scattering move_position in the read
                        // path across many places: read helper, tokenizer.rs,
                        // ... consolidate!
                        emitter.move_position(1);
                        xs = &xs[1..];
                    }
                    self.last_character_was_cr = false;
                }

                Ok(Some(xs))
            }
            None => {
                if self.last_character_was_cr {
                    self.last_character_was_cr = false;
                }
                Ok(None)
            }
        }
    }
}

/// A version of `match read_helper.read_char()` that "knows" about matched characters, so it can
/// produce a more efficient `read_until` call instead.
///
/// An extremely limited subset of match patterns is supported.
///
/// ```ignore
/// // I'm hitting multiple issues trying to get this test example to compile for doctests.
/// // Example: https://users.rust-lang.org/t/rustdoc-doctests-and-private-documentation/20955/6
/// use html5gum::{Reader, Tokenizer};
///
/// fn before<R: Reader>(slf: &mut Tokenizer<R>) {
///     match slf.reader.read_char() {
///         Some("<") => todo!(),
///         Some(x) => todo!(),
///         None => todo!()
///     }
/// }
///
/// fn after<R: Reader>(slf: &mut Tokenizer<R>) {
///     fast_read_char!(slf, reconsume_char, match xs {
///         Some("<") => {
///             todo!()
///         }
///         Some(x) => {
///             todo!()
///         }
///         None => {
///             todo!()
///         }
///     })
/// }
/// ```
macro_rules! fast_read_char {
    ($slf:expr, $reconsume_char:ident, match $read_char:ident {
        $(Some($($lit:literal)|*) => $arm:block)*
        Some($xs:ident) => $catchall:block
        None => $eof_catchall:block
    }) => { loop {
        let mut char_buf = [0; 4];
        let $read_char = match $reconsume_char.take() {
            // A character to reconsume, passed to the state by value.
            Some(Some(x)) => {
                char_buf[0] = x;
                Some(&char_buf[..1])
            }
            Some(None) => None,
            None => $crate::Tokenizer::read_until(
                &mut $slf.reader,
                &[ $($({
                    debug_assert_eq!($lit.len(), 1);
                    $lit[0]
                }),*),* ],
                &mut $slf.validator,
                &mut $slf.emitter,
                &mut char_buf,
            )?,
        };
        break match $read_char {
            $(
                Some($($lit)|*) => {
                    $arm
                }
            )*
                Some($xs) => {
                    // Prevent catch-all arm from using the machine_helper.
                    //
                    // State changes in catch-all arms are usually sign of a coding mistake. $xs
                    // may contain an arbitrary amount of characters, so it's more likely than not
                    // that the state is changed at the wrong read position.
                    //
                    // reconsume_in!() macro should not be used in this match arm either, as we can
                    // reconsume 2 characters at maximum, not a random $xs. Luckily that's kind of
                    // hard to do by accident.
                    #[allow(unused_variables)]
                    let _do_not_use = &mut $slf.machine_helper;
                    $catchall
                }
            None => {
                $eof_catchall
            }
        };
    } };
}

pub(crate) use fast_read_char;

macro_rules! slow_read_byte {
    ($slf:expr, $reconsume_char:ident, match c {
        $($tt:tt)*
    }) => { loop {
        let c = match $reconsume_char.take() {
            // A character to reconsume, passed to the state by value.
            Some(c) => c,
            None => $slf.read_byte()?,
        };
        break match c {
            $($tt)*
        };
    } };
}

pub(crate) use slow_read_byte;
