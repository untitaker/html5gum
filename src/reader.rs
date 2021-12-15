use crate::utils::ctostr;
use crate::Never;
use std::io::{self, BufRead, BufReader, Read};

/// An object that provides characters to the tokenizer.
///
/// See [`crate::Tokenizer::new`] for more information.
pub trait Reader {
    /// The error returned by this reader.
    type Error: std::error::Error;

    /// Return a new character from the input stream.
    ///
    /// The input stream does **not** have to be preprocessed in any way, it can contain standalone
    /// surrogates and have inconsistent newlines.
    fn read_char(&mut self) -> Result<Option<char>, Self::Error>;

    /// Attempt to read an entire string at once, either case-insensitively or not.
    ///
    /// `case_sensitive=false` means that characters of the input stream should be compared while
    /// ignoring ASCII-casing.
    ///
    /// It can be assumed that this function is never called with a string that contains `\r` or
    /// `\n`.
    ///
    /// If the next characters equal to `s`, this function consumes the respective characters from
    /// the input stream and returns `true`. If not, it does nothing and returns `false`.
    fn try_read_string(&mut self, s: &str, case_sensitive: bool) -> Result<bool, Self::Error>;

    /// Read an arbitrary amount of characters up until and including the next character that
    /// matches an array entry in `needle`.
    ///
    /// Repeatedly call `read_cb` with either:
    ///
    /// 1. A chunk of consumed characters that does not contain any characters from `needle`. The chunk can be arbitrarily large or small.
    /// 2. If the next character is included in `needle`, a string with just that character and nothing else.
    ///
    /// In other words, case 1 means "we didn't find the needle yet, but here's some read data",
    /// while case 2 means "we have found the needle".
    ///
    /// If `read_cb` is called with `None` (EOF) or a `needle` like in case 2, `read_until` returns
    /// that call's return value.
    ///
    /// The default implementation simply reads one character and calls `read_cb` with that
    /// character, ignoring the needle entirely. It is recommended to manually implement
    /// `read_until` if there is any sort of in-memory buffer where `memchr` can be run on.
    ///
    /// # Example
    ///
    /// Here is how [`StringReader`] behaves:
    ///
    /// ```rust
    /// use html5gum::{Reader, Readable};
    ///
    /// let mut reader = "hello world".to_reader();
    /// let mut eof = false;
    /// let mut chunks = Vec::new();
    /// while !eof {
    ///     reader.read_until(&[' ', 'r'], |xs| {
    ///         if let Some(xs) = xs {
    ///             chunks.push(xs.to_owned());
    ///         } else {
    ///             eof = true;
    ///         }
    ///     });
    /// }
    ///
    /// assert_eq!(chunks, &["hello", " ", "wo", "r", "ld"]);
    /// ```
    ///
    /// The inefficient default implementation produces:
    ///
    /// ```text
    /// ["h", "e", "l", "l", "o", " ", "w", "o", "r", "l", "d"]
    /// ```
    fn read_until<F, V>(&mut self, needle: &[char], mut read_cb: F) -> Result<V, Self::Error>
    where
        F: FnMut(Option<&str>) -> V,
    {
        let _needle = needle;

        match self.read_char()? {
            Some(x) => Ok(read_cb(Some(ctostr!(x)))),
            None => Ok(read_cb(None)),
        }
    }
}

/// An object that can be converted into a [`crate::Reader`].
///
/// For example, any utf8-string can be converted into a `StringReader`, such that
/// `Tokenizer::new("mystring")` and `Tokenizer::new(&String::new("foo"))` work.
pub trait Readable<'a> {
    /// The reader type to which this type should be converted.
    type Reader: Reader + 'a;

    /// Convert self to some sort of reader.
    fn to_reader(self) -> Self::Reader;
}

impl<'a, R: 'a + Reader> Readable<'a> for R {
    type Reader = Self;

    fn to_reader(self) -> Self::Reader {
        self
    }
}

/// A helper struct to seek forwards and backwards in strings. Used by the tokenizer to read HTML
/// from strings.
///
/// Example:
///
/// ```rust
/// use std::fmt::Write;
/// use html5gum::{Tokenizer, Token};
///
/// let html = "<title   >hello world</title>";
/// let mut new_html = String::new();
///
/// for token in Tokenizer::new(html).infallible() {
///     match token {
///         Token::StartTag(tag) => {
///             write!(new_html, "<{}>", tag.name).unwrap();
///         }
///         Token::String(hello_world) => {
///             write!(new_html, "{}", hello_world).unwrap();
///         }
///         Token::EndTag(tag) => {
///             write!(new_html, "</{}>", tag.name).unwrap();
///         }
///         _ => panic!("unexpected input"),
///     }
/// }
///
/// assert_eq!(new_html, "<title>hello world</title>");
/// ```
pub struct StringReader<'a> {
    input: &'a str,
    cursor: std::str::Chars<'a>,
    pos: usize,
}

impl<'a> StringReader<'a> {
    fn new(input: &'a str) -> Self {
        let cursor = input.chars();
        StringReader {
            input,
            cursor,
            pos: 0,
        }
    }
}

impl<'a> Reader for StringReader<'a> {
    type Error = Never;

    fn read_char(&mut self) -> Result<Option<char>, Self::Error> {
        let c = match self.cursor.next() {
            Some(c) => c,
            None => return Ok(None),
        };
        self.pos += c.len_utf8();
        Ok(Some(c))
    }

    fn read_until<F, V>(&mut self, needle: &[char], mut read_cb: F) -> Result<V, Self::Error>
    where
        F: FnMut(Option<&str>) -> V,
    {
        if let Some(input) = self.input.get(self.pos..) {
            if let Some(needle_pos) = fast_find(needle, input) {
                if needle_pos == 0 {
                    let needle = self.cursor.next().unwrap();
                    self.pos += needle.len_utf8();
                    Ok(read_cb(Some(ctostr!(needle))))
                } else {
                    self.pos += needle_pos;
                    let (s1, s2) = input.split_at(needle_pos);
                    self.cursor = s2.chars();
                    Ok(read_cb(Some(s1)))
                }
            } else {
                self.pos = self.input.len() + 1;
                self.cursor = "".chars();
                Ok(read_cb(Some(input)))
            }
        } else {
            Ok(read_cb(None))
        }
    }

    fn try_read_string(&mut self, s1: &str, case_sensitive: bool) -> Result<bool, Self::Error> {
        // we do not need to call validate_char here because `s` hopefully does not contain invalid
        // characters
        if let Some(s2) = self.input.get(self.pos..self.pos + s1.len()) {
            if s1 == s2 || (!case_sensitive && s1.eq_ignore_ascii_case(s2)) {
                self.pos += s1.len();
                self.cursor = self.input[self.pos..].chars();
                return Ok(true);
            }
        }

        Ok(false)
    }
}

impl<'a> Readable<'a> for &'a str {
    type Reader = StringReader<'a>;

    fn to_reader(self) -> Self::Reader {
        StringReader::new(self)
    }
}

impl<'a> Readable<'a> for &'a String {
    type Reader = StringReader<'a>;

    fn to_reader(self) -> Self::Reader {
        StringReader::new(self.as_str())
    }
}

/// A [`BufReadReader`] can be used to construct a tokenizer from any type that implements
/// `BufRead`.
///
/// Example:
///
/// ```rust
/// use std::io::BufReader;
/// use std::fmt::Write;
/// use html5gum::{Token, BufReadReader, Tokenizer};
///
/// let tokenizer = Tokenizer::new(BufReader::new("<title>hello world</title>".as_bytes()));
/// // or alternatively:
/// // tokenizer = Tokenizer::new(BufReadReader::new(BufReader::new("...".as_bytes())));
///
/// let mut new_html = String::new();
///
/// for token in tokenizer {
///     let token = token.unwrap();
///
///     match token {
///         Token::StartTag(tag) => {
///             write!(new_html, "<{}>", tag.name).unwrap();
///         }
///         Token::String(hello_world) => {
///             write!(new_html, "{}", hello_world).unwrap();
///         }
///         Token::EndTag(tag) => {
///             write!(new_html, "</{}>", tag.name).unwrap();
///         }
///         _ => panic!("unexpected input"),
///     }
///     
/// }
///
/// assert_eq!(new_html, "<title>hello world</title>");
/// ```
pub struct BufReadReader<R: BufRead> {
    line: String,
    line_pos: usize,
    reader: R,
}

impl<R: BufRead> BufReadReader<R> {
    /// Construct a new `BufReadReader` from any type that implements `BufRead`.
    pub fn new(reader: R) -> Self {
        BufReadReader {
            line: String::new(),
            line_pos: 0,
            reader,
        }
    }

    #[inline]
    fn get_remaining_line(&mut self) -> Result<&str, io::Error> {
        if self.line_pos < self.line.len() {
            return Ok(&self.line[self.line_pos..]);
        }

        self.line.clear();
        self.line_pos = 0;
        self.reader.read_line(&mut self.line)?;
        Ok(&self.line)
    }
}

impl<R: BufRead> Reader for BufReadReader<R> {
    type Error = io::Error;

    fn read_char(&mut self) -> Result<Option<char>, Self::Error> {
        let rv = self.get_remaining_line()?.chars().next();
        self.line_pos += rv.map(char::len_utf8).unwrap_or(1);
        Ok(rv)
    }

    fn try_read_string(&mut self, s1: &str, case_sensitive: bool) -> Result<bool, Self::Error> {
        debug_assert!(!s1.contains('\r'));
        debug_assert!(!s1.contains('\n'));

        if let Some(s2) = self.get_remaining_line()?.get(..s1.len()) {
            if s1 == s2 || (!case_sensitive && s1.eq_ignore_ascii_case(s2)) {
                self.line_pos += s1.len();
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn read_until<F, V>(&mut self, needle: &[char], mut read_cb: F) -> Result<V, Self::Error>
    where
        F: FnMut(Option<&str>) -> V,
    {
        let line = self.get_remaining_line()?;
        let rv;
        if !line.is_empty() {
            if let Some(needle_pos) = fast_find(needle, line) {
                if needle_pos == 0 {
                    let len = line.chars().next().unwrap().len_utf8();
                    rv = Ok(read_cb(Some(&line[..len])));
                    self.line_pos += len;
                } else {
                    rv = Ok(read_cb(Some(&line[..needle_pos])));
                    self.line_pos += needle_pos;
                }
            } else {
                rv = Ok(read_cb(Some(line)));
                self.line_pos += line.len();
            }
        } else {
            rv = Ok(read_cb(None));
        };

        rv
    }
}

impl<'a, R: Read + 'a> Readable<'a> for BufReader<R> {
    type Reader = BufReadReader<BufReader<R>>;

    fn to_reader(self) -> Self::Reader {
        BufReadReader::new(self)
    }
}

#[inline]
fn fast_find(needle: &[char], haystack: &str) -> Option<usize> {
    #[cfg(feature = "memchr")]
    if needle.iter().all(|x| x.is_ascii()) {
        if needle.len() == 3 {
            return memchr::memchr3(
                needle[0] as u8,
                needle[1] as u8,
                needle[2] as u8,
                haystack.as_bytes(),
            );
        } else if needle.len() == 2 {
            return memchr::memchr2(needle[0] as u8, needle[1] as u8, haystack.as_bytes());
        } else if needle.len() == 1 {
            return memchr::memchr(needle[0] as u8, haystack.as_bytes());
        }
    }

    haystack.find(needle)
}
