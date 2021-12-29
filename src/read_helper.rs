use crate::Emitter;
use crate::Error;
use crate::Reader;

use crate::utils::{control_pat, noncharacter_pat, surrogate_pat};

pub(crate) struct ReadHelper<R: Reader> {
    reader: R,
    last_character_was_cr: bool,
    to_reconsume: Stack8<Option<u8>>,
    last_4_bytes: [u8; 4],
}

impl<R: Reader> ReadHelper<R> {
    pub(crate) fn new(reader: R) -> Self {
        ReadHelper {
            reader,
            last_character_was_cr: false,
            to_reconsume: Default::default(),
            last_4_bytes: [0; 4],
        }
    }

    pub(crate) fn read_byte<E: Emitter>(
        &mut self,
        emitter: &mut E,
    ) -> Result<Option<u8>, R::Error> {
        let mut c = match self.to_reconsume.pop() {
            Some(c) => return Ok(c),
            None => self.reader.read_byte(),
        };

        if self.last_character_was_cr && matches!(c, Ok(Some(b'\n'))) {
            self.last_character_was_cr = false;
            return self.read_byte(emitter);
        }

        if matches!(c, Ok(Some(b'\r'))) {
            self.last_character_was_cr = true;
            c = Ok(Some(b'\n'));
        } else {
            self.last_character_was_cr = false;
        }

        if let Ok(Some(x)) = c {
            Self::validate_char(emitter, &mut self.last_4_bytes, x);
        }

        c
    }

    #[inline]
    pub(crate) fn try_read_string(
        &mut self,
        mut s: &str,
        case_sensitive: bool,
    ) -> Result<bool, R::Error> {
        debug_assert!(!s.is_empty());
        debug_assert!(!s.contains('\r'));

        let to_reconsume_bak = self.to_reconsume;
        let mut bytes = s.as_bytes().iter();
        while let Some(c) = self.to_reconsume.pop() {
            if let (Some(x), Some(&x2)) = (c, bytes.next()) {
                if x == x2 || (!case_sensitive && x.to_ascii_lowercase() == x2.to_ascii_lowercase())
                {
                    s = &s[1..];
                    continue;
                }
            }

            self.to_reconsume = to_reconsume_bak;
            return Ok(false);
        }

        if s.is_empty() || self.reader.try_read_string(s.as_bytes(), case_sensitive)? {
            self.last_character_was_cr = false;
            self.last_4_bytes = [0; 4];
            Ok(true)
        } else {
            self.to_reconsume = to_reconsume_bak;
            Ok(false)
        }
    }

    #[inline]
    pub(crate) fn read_until<'b, E>(
        &'b mut self,
        needle: &[u8],
        emitter: &mut E,
        char_buf: &'b mut [u8; 4],
    ) -> Result<Option<&'b [u8]>, R::Error>
    where
        E: Emitter,
    {
        match self.to_reconsume.pop() {
            Some(Some(x)) => {
                return Ok(Some({
                    char_buf[0] = x;
                    &char_buf[..1]
                }))
            }
            Some(None) => return Ok(None),
            None => (),
        }

        let last_character_was_cr = &mut self.last_character_was_cr;

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
                *last_character_was_cr = true;
                Self::validate_char(emitter, &mut self.last_4_bytes, b'\n');
                Ok(Some(b"\n"))
            }
            Some(mut xs) => {
                for x in xs {
                    Self::validate_char(emitter, &mut self.last_4_bytes, *x);
                }

                if *last_character_was_cr && xs.starts_with(b"\n") {
                    xs = &xs[1..];
                }

                *last_character_was_cr = false;
                Ok(Some(xs))
            }
            None => {
                *last_character_was_cr = false;
                Ok(None)
            }
        }
    }

    #[inline]
    pub(crate) fn unread_char(&mut self, c: Option<char>) {
        self.to_reconsume.push(c.map(|x| x as u8));
    }

    #[inline]
    fn validate_char<E: Emitter>(emitter: &mut E, last_4_bytes: &mut [u8; 4], next_byte: u8) {
        last_4_bytes.rotate_left(1);
        last_4_bytes[3] = next_byte;

        // convert a u32 containing the last 4 bytes to the corresponding unicode scalar value, if
        // there's any.
        //
        // `last_4_bytes` is utf8-encoded character (or trunchated garbage), while `char_c` is a
        // `char`.
        //
        // ideally this function would pattern match on `last_4_bytes` directly.
        let char_c = if matches!(last_4_bytes, [0, 0, 0, _]) {
            last_4_bytes[3] as char
        } else {
            let first_non_null_byte = last_4_bytes[..].iter().position(|&x| x != b'\0').unwrap_or(0);
            match std::str::from_utf8(&last_4_bytes[first_non_null_byte..]) {
                Ok(x) => x.chars().next().unwrap(),
                Err(_) => return,
            }
        };

        if char_c.is_ascii() {
            *last_4_bytes = [0; 4];
        }

        match char_c as u32 {
            surrogate_pat!() => {
                emitter.emit_error(Error::SurrogateInInputStream);
            }
            noncharacter_pat!() => {
                emitter.emit_error(Error::NoncharacterInInputStream);
            }
            // control without whitespace or nul
            x @ control_pat!()
                if !matches!(x, 0x0000 | 0x0009 | 0x000a | 0x000c | 0x000d | 0x0020) =>
            {
                emitter.emit_error(Error::ControlCharacterInInputStream);
            }
            _ => (),
        }
    }
}

// this is a stack that can hold 0 to 8 Ts
#[derive(Debug, Default, Clone, Copy)]
struct Stack8<T: Copy> {
    buf: [T; 8],
    len: usize,
}

impl<T: Copy> Stack8<T> {
    #[inline]
    fn push(&mut self, c: T) {
        self.buf[self.len] = c;
        self.len += 1;
    }

    #[inline]
    fn pop(&mut self) -> Option<T> {
        if self.len > 0 {
            self.len -= 1;
            Some(self.buf[self.len])
        } else {
            None
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
