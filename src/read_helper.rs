use crate::Emitter;
use crate::Error;
use crate::Reader;

use crate::utils::{control_pat, ctostr, noncharacter_pat, surrogate_pat};

pub(crate) struct ReadHelper<R: Reader> {
    reader: R,
    to_reconsume: Stack2<Option<char>>,
}

impl<R: Reader> ReadHelper<R> {
    pub(crate) fn new(reader: R) -> Self {
        ReadHelper {
            reader,
            to_reconsume: Default::default(),
        }
    }

    #[inline]
    pub(crate) fn read_char<E: Emitter>(
        &mut self,
        emitter: &mut E,
    ) -> Result<Option<char>, R::Error> {
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
            Self::validate_char(emitter, c);
        }

        Ok(Some(c))
    }

    #[inline]
    pub(crate) fn try_read_string(
        &mut self,
        mut s: &str,
        case_sensitive: bool,
    ) -> Result<bool, R::Error> {
        debug_assert!(!s.is_empty());

        let to_reconsume_bak = self.to_reconsume;
        let mut chars = s.chars();
        while let Some(c) = self.to_reconsume.pop() {
            if let (Some(x), Some(x2)) = (c, chars.next()) {
                if x == x2 || (!case_sensitive && x.to_ascii_lowercase() == x2.to_ascii_lowercase())
                {
                    s = &s[x.len_utf8()..];
                    continue;
                }
            }

            self.to_reconsume = to_reconsume_bak;
            return Ok(false);
        }

        self.reader.try_read_string(s, case_sensitive)
    }

    #[inline]
    pub(crate) fn read_until<V, F: FnMut(Option<&str>, &mut E) -> V, E: Emitter>(
        &mut self,
        needle: &[char],
        emitter: &mut E,
        mut read_cb: F,
    ) -> Result<V, R::Error> {
        match self.to_reconsume.pop() {
            Some(Some(x)) => Ok(read_cb(Some(ctostr!(x)), emitter)),
            Some(None) => Ok(read_cb(None, emitter)),
            None => {
                let mut last_character_was_cr = false;

                loop {
                    let rv = self.reader.read_until(needle, |xs| {
                        match xs {
                            Some(xs) if xs.find(&['\r', '\n'][..]).is_some() => {
                                let mut last_rv = None;

                                // TODO: slow
                                for x in xs.chars() {
                                    if x == '\r' {
                                        last_rv = Some(read_cb(Some("\n"), emitter));
                                        last_character_was_cr = true;
                                    } else if x == '\n' {
                                        if !last_character_was_cr {
                                            last_rv = Some(read_cb(Some("\n"), emitter));
                                        }
                                        last_character_was_cr = false;
                                    } else {
                                        Self::validate_char(emitter, x);
                                        last_rv = Some(read_cb(Some(ctostr!(x)), emitter));
                                        last_character_was_cr = false;
                                    }
                                }
                                last_rv
                            }
                            xs => {
                                if let Some(xs) = xs {
                                    for x in xs.chars() {
                                        Self::validate_char(emitter, x);
                                    }
                                }
                                last_character_was_cr = false;
                                Some(read_cb(xs, emitter))
                            }
                        }
                    })?;

                    if !last_character_was_cr {
                        if let Some(rv) = rv {
                            break Ok(rv);
                        }
                    }
                }
            }
        }
    }

    #[inline]
    pub(crate) fn unread_char(&mut self, c: Option<char>) {
        self.to_reconsume.push(c);
    }

    #[inline]
    fn validate_char<E: Emitter>(emitter: &mut E, c: char) {
        match c as u32 {
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

// this is a stack that can hold 0 to 2 Ts
#[derive(Debug, Default, Clone, Copy)]
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
}

macro_rules! produce_needle {
    (($($acc:tt)*); Some($var:ident @ ($($pattern:tt)*)) $($rest:tt)*) => {
        $crate::read_helper::produce_needle!(
            ($($acc)*);
            Some($($pattern)*)
            $($rest)*
        )
    };

    (($($acc:tt)*); Some($($x:literal)|*) $($rest:tt)*) => {
        $crate::read_helper::produce_needle!((
            $($acc)*
            $(
            {
                debug_assert_eq!($x.len(), 1);
                $x.chars().next().unwrap()
            },
            )*
        ); $($rest)*)
    };
    (($($acc:tt)*); Some($x:ident) $($rest:tt)*) => {
        $crate::read_helper::produce_needle!(($($acc)*); $($rest)*)
    };
    (($($acc:tt)*); c $($rest:tt)*) => {
        $crate::read_helper::produce_needle!(($($acc)*); $($rest)*)
    };
    (($($acc:tt)*); None $($rest:tt)*) => {
        $crate::read_helper::produce_needle!(($($acc)*); $($rest)*)
    };
    (($($acc:tt)*); , $($rest:tt)*) => {
        $crate::read_helper::produce_needle!(($($acc)*); $($rest)*)
    };
    (($($acc:tt)*); => $($rest:tt)*) => {
        $crate::read_helper::produce_needle!(($($acc)*); $($rest)*)
    };
    (($($acc:tt)*); { $($garbage:tt)* } $($rest:tt)*) => {
        $crate::read_helper::produce_needle!(($($acc)*); $($rest)*)
    };
    (($($acc:tt)*); ( $($pattern:tt)* ) $($rest:tt)*) => {
        $crate::read_helper::produce_needle!(
            ($($acc)*);
            $($pattern)* $($rest)*
        )
    };
    (($($acc:tt)*); ) => {
        [ $($acc)* ]
    };
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
///     fast_read_char!(slf, emitter, match READ_CHAR {
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
    ($slf:expr, $emitter:ident, match READ_CHAR { $($arms:tt)* }) => {
        $slf.reader.read_until(
            &$crate::read_helper::produce_needle!((); $($arms)*),
            &mut $slf.emitter,
            |xs, $emitter| match xs {
                $($arms)*
            }
        )
    };
}

pub(crate) use fast_read_char;
pub(crate) use produce_needle;
