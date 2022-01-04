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
        // convert a u32 containing the last 4 bytes to the corresponding unicode scalar value, if
        // there's any.
        //
        // `last_4_bytes` is utf8-encoded character (or trunchated garbage).

        if next_byte < 128 {
            // ascii
            *last_4_bytes = 0;
            Self::validate_last_4_bytes(emitter, next_byte as u32);
        } else if next_byte >= 192 {
            // (non-ascii) character boundary
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
        // ' | '.join(map(str, sorted([int.from_bytes(chr(x).encode("utf8"), 'big') for x in nonchars])))
        match last_4_bytes {
            15710096 | 15710097 | 15710098 | 15710099 | 15710100 | 15710101 | 15710102
            | 15710103 | 15710104 | 15710105 | 15710106 | 15710107 | 15710108 | 15710109
            | 15710110 | 15710111 | 15710112 | 15710113 | 15710114 | 15710115 | 15710116
            | 15710117 | 15710118 | 15710119 | 15710120 | 15710121 | 15710122 | 15710123
            | 15710124 | 15710125 | 15710126 | 15710127 | 15712190 | 15712191 | 4037001150
            | 4037001151 | 4038049726 | 4038049727 | 4039098302 | 4039098303 | 4052729790
            | 4052729791 | 4053778366 | 4053778367 | 4054826942 | 4054826943 | 4055875518
            | 4055875519 | 4069507006 | 4069507007 | 4070555582 | 4070555583 | 4071604158
            | 4071604159 | 4072652734 | 4072652735 | 4086284222 | 4086284223 | 4087332798
            | 4087332799 | 4088381374 | 4088381375 | 4089429950 | 4089429951 | 4103061438
            | 4103061439 => {
                emitter.emit_error(Error::NoncharacterInInputStream);
            }
            1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 11 | 13 | 14 | 15 | 16 | 17 | 18 | 19 | 20 | 21
            | 22 | 23 | 24 | 25 | 26 | 27 | 28 | 29 | 30 | 31 | 127 | 49792 | 49793 | 49794
            | 49795 | 49796 | 49797 | 49798 | 49799 | 49800 | 49801 | 49802 | 49803 | 49804
            | 49805 | 49806 | 49807 | 49808 | 49809 | 49810 | 49811 | 49812 | 49813 | 49814
            | 49815 | 49816 | 49817 | 49818 | 49819 | 49820 | 49821 | 49822 | 49823 => {
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
