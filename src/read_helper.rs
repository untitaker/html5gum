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
            Some(Some(x)) => return Ok(read_cb(Some(ctostr!(x)), emitter)),
            Some(None) => return Ok(read_cb(None, emitter)),
            None => (),
        }

        let mut last_character_was_cr = false;

        loop {
            let rv = self.reader.read_until(needle, |xs| {
                let xs = match xs {
                    Some(xs) => xs,
                    None => {
                        last_character_was_cr = false;
                        return read_cb(None, emitter);
                    }
                };

                let mut last_i = 0;
                if last_character_was_cr && xs.starts_with('\n') {
                    last_i = 1;
                }

                for (i, _) in xs.match_indices('\r') {
                    let xs2 = &xs[last_i..i];
                    for x in xs2.chars() {
                        Self::validate_char(emitter, x);
                    }
                    read_cb(Some(xs2), emitter);
                    read_cb(Some("\n"), emitter);
                    last_i = i + 1;
                    if xs.as_bytes().get(last_i) == Some(&b'\n') {
                        last_i += 1;
                    }
                }

                let xs2 = &xs[last_i..];
                for x in xs2.chars() {
                    Self::validate_char(emitter, x);
                }
                last_character_was_cr = xs.ends_with('\r');
                read_cb(Some(xs2), emitter)
            });

            if !last_character_was_cr {
                break rv;
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
    ($slf:expr, $emitter:ident, $machine_helper:ident, match $read_char:ident {
        $(Some($($lit:literal)|*) => $arm:block)*
        Some($xs:ident) => $catchall:block
        None => $eof_catchall:block
    }) => {
        $slf.reader.read_until(
            &[ $($({
                debug_assert_eq!($lit.len(), 1);
                $lit.chars().next().unwrap()
            }),*),* ],
            &mut $slf.emitter,
            |$read_char, $emitter| match $read_char {
                $(Some($($lit)|*) => $arm)*
                Some($xs) => {
                    // Prevent catch-all arm from using the machine_helper.
                    //
                    // State changes in catch-all arms are usually sign of a coding mistake. $xs
                    // may contain an arbitrary amount of characters, so it's more likely than not
                    // that the state is changed at the wrong read position.
                    #[allow(unused_variables)]
                    let $machine_helper = ();
                    $catchall
                }
                None => $eof_catchall
            }
        )
    };
}

pub(crate) use fast_read_char;
