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
        for &x in next_bytes {
            Self::validate_byte(emitter, last_4_bytes, x);
        }
    }

    #[inline]
    fn validate_byte<E: Emitter>(emitter: &mut E, last_4_bytes: &mut u32, next_byte: u8) {
        if next_byte < 128 {
            *last_4_bytes = 0;
            Self::validate_last_4_bytes(emitter, next_byte as u32);
        } else if next_byte >= 192 {
            *last_4_bytes = next_byte as u32;
        } else {
            *last_4_bytes <<= 8;
            *last_4_bytes |= next_byte as u32;
            Self::validate_last_4_bytes(emitter, *last_4_bytes);
        }
    }

    #[inline]
    fn validate_last_4_bytes<E: Emitter>(emitter: &mut E, last_4_bytes: u32) {
        // generated with Python 3:
        // ' | '.join(map(hex, sorted([int.from_bytes(chr(x).encode("utf8"), 'big') for x in nonchars])))
        match last_4_bytes {
            0xefb790 | 0xefb791 | 0xefb792 | 0xefb793 | 0xefb794 | 0xefb795 | 0xefb796
            | 0xefb797 | 0xefb798 | 0xefb799 | 0xefb79a | 0xefb79b | 0xefb79c | 0xefb79d
            | 0xefb79e | 0xefb79f | 0xefb7a0 | 0xefb7a1 | 0xefb7a2 | 0xefb7a3 | 0xefb7a4
            | 0xefb7a5 | 0xefb7a6 | 0xefb7a7 | 0xefb7a8 | 0xefb7a9 | 0xefb7aa | 0xefb7ab
            | 0xefb7ac | 0xefb7ad | 0xefb7ae | 0xefb7af | 0xefbfbe | 0xefbfbf | 0xf09fbfbe
            | 0xf09fbfbf | 0xf0afbfbe | 0xf0afbfbf | 0xf0bfbfbe | 0xf0bfbfbf | 0xf18fbfbe
            | 0xf18fbfbf | 0xf19fbfbe | 0xf19fbfbf | 0xf1afbfbe | 0xf1afbfbf | 0xf1bfbfbe
            | 0xf1bfbfbf | 0xf28fbfbe | 0xf28fbfbf | 0xf29fbfbe | 0xf29fbfbf | 0xf2afbfbe
            | 0xf2afbfbf | 0xf2bfbfbe | 0xf2bfbfbf | 0xf38fbfbe | 0xf38fbfbf | 0xf39fbfbe
            | 0xf39fbfbf | 0xf3afbfbe | 0xf3afbfbf | 0xf3bfbfbe | 0xf3bfbfbf | 0xf48fbfbe
            | 0xf48fbfbf => {
                emitter.emit_error(Error::NoncharacterInInputStream);
            }
            0x1 | 0x2 | 0x3 | 0x4 | 0x5 | 0x6 | 0x7 | 0x8 | 0xb | 0xd | 0xe | 0xf | 0x10 | 0x11
            | 0x12 | 0x13 | 0x14 | 0x15 | 0x16 | 0x17 | 0x18 | 0x19 | 0x1a | 0x1b | 0x1c | 0x1d
            | 0x1e | 0x1f | 0x7f | 0xc280 | 0xc281 | 0xc282 | 0xc283 | 0xc284 | 0xc285 | 0xc286
            | 0xc287 | 0xc288 | 0xc289 | 0xc28a | 0xc28b | 0xc28c | 0xc28d | 0xc28e | 0xc28f
            | 0xc290 | 0xc291 | 0xc292 | 0xc293 | 0xc294 | 0xc295 | 0xc296 | 0xc297 | 0xc298
            | 0xc299 | 0xc29a | 0xc29b | 0xc29c | 0xc29d | 0xc29e | 0xc29f => {
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
