use crate::char_validator::CharValidator;
use crate::Emitter;
use crate::Reader;

#[derive(Debug)]
pub(crate) struct ReadHelper<R: Reader> {
    reader: R,
    last_character_was_cr: bool,
    #[allow(clippy::option_option)]
    to_reconsume: Option<Option<u8>>,
}

impl<R: Reader> ReadHelper<R> {
    pub(crate) fn new(reader: R) -> Self {
        ReadHelper {
            reader,
            last_character_was_cr: false,
            to_reconsume: None,
        }
    }

    pub(crate) fn read_byte<E: Emitter>(
        &mut self,
        char_validator: &mut CharValidator,
        emitter: &mut E,
    ) -> Result<Option<u8>, R::Error> {
        if let Some(c) = self.to_reconsume.take() {
            return Ok(c);
        }

        let mut c = self.reader.read_byte();
        if self.last_character_was_cr && matches!(c, Ok(Some(b'\n'))) {
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

    pub(crate) fn try_read_string(
        &mut self,
        char_validator: &mut CharValidator,
        mut s: &str,
        case_sensitive: bool,
    ) -> Result<bool, R::Error> {
        debug_assert!(!s.is_empty());
        debug_assert!(!s.contains('\r'));

        let to_reconsume_bak = self.to_reconsume;
        let mut bytes = s.as_bytes().iter();
        if let Some(c) = self.to_reconsume.take() {
            match (c, bytes.next()) {
                (Some(x), Some(&x2))
                    if x == x2
                        || (!case_sensitive
                            && x.to_ascii_lowercase() == x2.to_ascii_lowercase()) =>
                {
                    s = &s[1..];
                }
                _ => {
                    self.to_reconsume = to_reconsume_bak;
                    return Ok(false);
                }
            }
        }

        if s.is_empty() || self.reader.try_read_string(s.as_bytes(), case_sensitive)? {
            self.last_character_was_cr = false;
            char_validator.reset();
            Ok(true)
        } else {
            self.to_reconsume = to_reconsume_bak;
            Ok(false)
        }
    }

    pub(crate) fn read_until<'b, E>(
        &'b mut self,
        needle: &[u8],
        char_validator: &mut CharValidator,
        emitter: &mut E,
        char_buf: &'b mut [u8; 4],
    ) -> Result<Option<&'b [u8]>, R::Error>
    where
        E: Emitter,
    {
        const MAX_NEEDLE_LEN: usize = 13;

        match self.to_reconsume.take() {
            Some(Some(x)) => {
                return Ok(Some({
                    char_buf[0] = x;
                    &char_buf[..1]
                }))
            }
            Some(None) => return Ok(None),
            None => (),
        }

        let mut needle2 = [b'\0'; MAX_NEEDLE_LEN];
        // Assert that we will have space for adding \r
        // If not, just bump MAX_NEEDLE_LEN
        debug_assert!(needle.len() < needle2.len());
        needle2[..needle.len()].copy_from_slice(needle);
        needle2[needle.len()] = b'\r';
        let needle2_slice = &needle2[..=needle.len()];

        match self.reader.read_until(needle2_slice, char_buf)? {
            Some(b"\r") => {
                self.last_character_was_cr = true;
                char_validator.validate_byte(emitter, b'\n');
                Ok(Some(b"\n"))
            }
            Some(mut xs) => {
                char_validator.validate_bytes(emitter, xs);

                if self.last_character_was_cr && xs.starts_with(b"\n") {
                    xs = &xs[1..];
                }

                self.last_character_was_cr = false;
                Ok(Some(xs))
            }
            None => {
                self.last_character_was_cr = false;
                Ok(None)
            }
        }
    }

    #[inline]
    pub(crate) fn unread_byte(&mut self, c: Option<u8>) {
        self.to_reconsume = Some(c);
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
///     fast_read_char!(slf, emitter, match xs {
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
    ($slf:expr, match $read_char:ident {
        $(Some($($lit:literal)|*) => $arm:block)*
        Some($xs:ident) => $catchall:block
        None => $eof_catchall:block
    }) => {{
        let mut char_buf = [0; 4];
        let $read_char = $slf.reader.read_until(
            &[ $($({
                debug_assert_eq!($lit.len(), 1);
                $lit[0]
            }),*),* ],
            &mut $slf.validator,
            &mut $slf.emitter,
            &mut char_buf,
        )?;
        match $read_char {
            $(Some($($lit)|*) => $arm)*
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
            None => $eof_catchall
        }
    }};
}

pub(crate) use fast_read_char;
