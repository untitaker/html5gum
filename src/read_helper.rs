use crate::Emitter;
use crate::Error;
use crate::Reader;

pub(crate) struct ReadHelper<R: Reader> {
    reader: R,
    last_character_was_cr: bool,
    to_reconsume: Option<Option<u8>>,
    last_4_bytes: u32,
}

impl<R: Reader> ReadHelper<R> {
    pub(crate) fn new(reader: R) -> Self {
        ReadHelper {
            reader,
            last_character_was_cr: false,
            to_reconsume: Default::default(),
            last_4_bytes: 0,
        }
    }

    pub(crate) fn read_byte<E: Emitter>(
        &mut self,
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
            Self::validate_byte(emitter, &mut self.last_4_bytes, x);
        }

        c
    }

    pub(crate) fn try_read_string(
        &mut self,
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
            self.last_4_bytes = 0;
            Ok(true)
        } else {
            self.to_reconsume = to_reconsume_bak;
            Ok(false)
        }
    }

    pub(crate) fn read_until<'b, E>(
        &'b mut self,
        needle: &[u8],
        emitter: &mut E,
        char_buf: &'b mut [u8; 4],
    ) -> Result<Option<&'b [u8]>, R::Error>
    where
        E: Emitter,
    {
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

        const MAX_NEEDLE_LEN: usize = 13;
        let mut needle2 = [b'\0'; MAX_NEEDLE_LEN];
        // Assert that we will have space for adding \r
        // If not, just bump MAX_NEEDLE_LEN
        debug_assert!(needle.len() < needle2.len());
        needle2[..needle.len()].copy_from_slice(needle);
        needle2[needle.len()] = b'\r';
        let needle2_slice = &needle2[..needle.len() + 1];

        match self.reader.read_until(needle2_slice, char_buf)? {
            Some(b"\r") => {
                self.last_character_was_cr = true;
                Self::validate_byte(emitter, &mut self.last_4_bytes, b'\n');
                Ok(Some(b"\n"))
            }
            Some(mut xs) => {
                Self::validate_bytes(emitter, &mut self.last_4_bytes, xs);

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

    #[inline]
    fn validate_bytes<E: Emitter>(emitter: &mut E, last_4_bytes: &mut u32, next_bytes: &[u8]) {
        if let Ok(xs) = std::str::from_utf8(next_bytes) {
            *last_4_bytes = 0;
            for x in xs.chars() {
                Self::validate_char(emitter, x);
            }
        } else {
            for &x in next_bytes {
                Self::validate_byte(emitter, last_4_bytes, x);
            }
        }
    }

    #[inline]
    fn validate_byte<E: Emitter>(emitter: &mut E, last_4_bytes: &mut u32, next_byte: u8) {
        // convert a u32 containing the last 4 bytes to the corresponding unicode scalar value, if
        // there's any.
        //
        // `last_4_bytes` is utf8-encoded character (or trunchated garbage), while `char_c` is a
        // `char`
        //
        // ideally this function would pattern match on `last_4_bytes` directly.

        if next_byte < 128 {
            // ascii
            *last_4_bytes = 0;
            Self::validate_char(emitter, next_byte as char);
        } else if next_byte >= 192 {
            // (non-ascii) character boundary
            *last_4_bytes = next_byte as u32;
        } else {
            *last_4_bytes <<= 8;
            *last_4_bytes |= next_byte as u32;
            if let Ok(x) = std::str::from_utf8(&last_4_bytes.to_be_bytes()[..]) {
                // last_4_bytes contains a valid character, potentially prefixed by some nullbytes.
                // get the last character
                //
                // we rely on the other branches to ensure no other state can occur
                Self::validate_char(emitter, x.chars().rev().next().unwrap());
            } else {
                // last_4_bytes contains truncated utf8 and it's not time to validate a character
                // yet
            }
        }
    }

    #[inline]
    fn validate_char<E: Emitter>(emitter: &mut E, char_c: char) {
        match char_c {
            // TODO: we cannot validate surrogates
            //'\u{d800}'..='\u{dfff}' => {
            //emitter.emit_error(Error::SurrogateInInputStream);
            //}
            //
            '\u{fdd0}'..='\u{fdef}'
            | '\u{fffe}'
            | '\u{ffff}'
            | '\u{1fffe}'
            | '\u{1ffff}'
            | '\u{2fffe}'
            | '\u{2ffff}'
            | '\u{3fffe}'
            | '\u{3ffff}'
            | '\u{4fffe}'
            | '\u{4ffff}'
            | '\u{5fffe}'
            | '\u{5ffff}'
            | '\u{6fffe}'
            | '\u{6ffff}'
            | '\u{7fffe}'
            | '\u{7ffff}'
            | '\u{8fffe}'
            | '\u{8ffff}'
            | '\u{9fffe}'
            | '\u{9ffff}'
            | '\u{afffe}'
            | '\u{affff}'
            | '\u{bfffe}'
            | '\u{bffff}'
            | '\u{cfffe}'
            | '\u{cffff}'
            | '\u{dfffe}'
            | '\u{dffff}'
            | '\u{efffe}'
            | '\u{effff}'
            | '\u{ffffe}'
            | '\u{fffff}'
            | '\u{10fffe}'
            | '\u{10ffff}' => {
                emitter.emit_error(Error::NoncharacterInInputStream);
            }
            // control without whitespace or nul
            x @ ('\u{1}'..='\u{1f}' | '\u{7f}'..='\u{9f}')
                if !matches!(x, '\u{9}' | '\u{a}' | '\u{c}' | '\u{20}') =>
            {
                emitter.emit_error(Error::ControlCharacterInInputStream);
            }
            _ => (),
        }
    }
}

/// A version of `match read_helper.read_char()` that "knows" about matched characters, so it can
/// produce a more efficient `read_until` call instead.
///
/// An extremely limited subset of match patterns is supported.
///
/// ```rust
/// # This documentation example isnt actually running. See
/// # https://users.rust-lang.org/t/rustdoc-doctests-and-private-documentation/20955/6
///
/// use crate::{Reader, Tokenizer};
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
