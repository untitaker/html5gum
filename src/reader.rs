use crate::Never;
use std::cmp::min;
use std::fs::File;
use std::io::{self, Read};

/// An object that provides characters to the tokenizer.
///
/// See [`crate::Tokenizer::new`] for more information.
pub trait Reader {
    /// The error returned by this reader.
    type Error: std::error::Error;

    /// Return a new byte from the input stream.
    ///
    /// The input stream does **not** have to be preprocessed in any way, it can contain standalone
    /// surrogates and have inconsistent newlines.
    fn read_byte(&mut self) -> Result<Option<u8>, Self::Error>;

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
    fn try_read_string(&mut self, s: &[u8], case_sensitive: bool) -> Result<bool, Self::Error>;

    /// Read an arbitrary amount of characters up until and including the next character that
    /// matches an array entry in `needle`.
    ///
    /// Return either:
    ///
    /// 1. A chunk of consumed characters that does not contain any characters from `needle`. The chunk can be arbitrarily large or small.
    /// 2. If the next character is included in `needle`, a string with just that character and nothing else.
    ///
    /// In other words, case 1 means "we didn't find the needle yet, but here's some read data",
    /// while case 2 means "we have found the needle".
    ///
    /// The default implementation simply reads one character and calls `read_cb` with that
    /// character, ignoring the needle entirely. It is recommended to manually implement
    /// `read_until` if there is any sort of in-memory buffer where `memchr` can be run on.
    ///
    /// The return value is usually borrowed from underlying buffers. If that's not possible, a
    /// small buffer is provided as `char_buf` to put a single character into.
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
    ///     let mut char_buf = [0; 4];
    ///     let xs = reader.read_until(&[b' ', b'r'], &mut char_buf).unwrap();
    ///     if let Some(xs) = xs {
    ///         chunks.push(std::str::from_utf8(xs).unwrap().to_owned());
    ///     } else {
    ///         eof = true;
    ///     }
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
    fn read_until<'b>(
        &'b mut self,
        needle: &[u8],
        char_buf: &'b mut [u8; 4],
    ) -> Result<Option<&'b [u8]>, Self::Error> {
        let _needle = needle;

        match self.read_byte()? {
            Some(x) => {
                char_buf[0] = x;
                Ok(Some(&char_buf[..1]))
            }
            None => Ok(None),
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
///             write!(new_html, "<{}>", String::from_utf8_lossy(&tag.name)).unwrap();
///         }
///         Token::String(hello_world) => {
///             write!(new_html, "{}", String::from_utf8_lossy(&hello_world)).unwrap();
///         }
///         Token::EndTag(tag) => {
///             write!(new_html, "</{}>", String::from_utf8_lossy(&tag.name)).unwrap();
///         }
///         _ => panic!("unexpected input"),
///     }
/// }
///
/// assert_eq!(new_html, "<title>hello world</title>");
/// ```
pub struct StringReader<'a> {
    input: &'a [u8],
}

impl<'a> StringReader<'a> {
    fn new(input: &'a [u8]) -> Self {
        StringReader { input }
    }
}

impl<'a> Reader for StringReader<'a> {
    type Error = Never;

    fn read_byte(&mut self) -> Result<Option<u8>, Self::Error> {
        if self.input.is_empty() {
            Ok(None)
        } else {
            let rv = self.input[0];
            self.input = &self.input[1..];
            Ok(Some(rv))
        }
    }

    fn read_until<'b>(
        &'b mut self,
        needle: &[u8],
        _: &'b mut [u8; 4],
    ) -> Result<Option<&'b [u8]>, Self::Error> {
        if self.input.is_empty() {
            return Ok(None);
        }

        if let Some(needle_pos) = fast_find(needle, self.input) {
            if needle_pos == 0 {
                let (rv, new_input) = self.input.split_at(1);
                self.input = new_input;
                Ok(Some(rv))
            } else {
                let (rv, new_input) = self.input.split_at(needle_pos);
                self.input = new_input;
                Ok(Some(rv))
            }
        } else {
            let rv = self.input;
            self.input = b"";
            Ok(Some(rv))
        }
    }

    fn try_read_string(&mut self, s1: &[u8], case_sensitive: bool) -> Result<bool, Self::Error> {
        // we do not need to call validate_char here because `s` hopefully does not contain invalid
        // characters
        if let Some(s2) = self.input.get(..s1.len()) {
            if s1 == s2 || (!case_sensitive && s1.eq_ignore_ascii_case(s2)) {
                self.input = &self.input[s1.len()..];
                return Ok(true);
            }
        }

        Ok(false)
    }
}

impl<'a> Readable<'a> for &'a str {
    type Reader = StringReader<'a>;

    fn to_reader(self) -> Self::Reader {
        StringReader::new(self.as_bytes())
    }
}

impl<'a> Readable<'a> for &'a String {
    type Reader = StringReader<'a>;

    fn to_reader(self) -> Self::Reader {
        StringReader::new(self.as_bytes())
    }
}

impl<'a> Readable<'a> for &'a Vec<u8> {
    type Reader = StringReader<'a>;

    fn to_reader(self) -> Self::Reader {
        StringReader::new(self.as_slice())
    }
}

impl<'a> Readable<'a> for &'a [u8] {
    type Reader = StringReader<'a>;

    fn to_reader(self) -> Self::Reader {
        StringReader::new(self)
    }
}

/// A [`IoReader`] can be used to construct a tokenizer from any type that implements
/// `std::io::Read`.
///
/// Because of trait impl conflicts, `IoReader` needs to be explicitly constructed. The exception
/// to that is `File`, which can be directly passed to `Tokenizer::new`.
///
/// When passing `Read`-types into html5gum, no I/O buffering is required. html5gum maintains its
/// own read-buffer (16kb, heap-allocated) such that it can be accessed directly. Put more simply,
/// it's wasteful to wrap your `File` in a `std::io::BufReader` before passing it to html5gum.
///
/// Example:
///
/// ```rust
/// use std::fmt::Write;
/// use html5gum::{Token, IoReader, Tokenizer};
///
/// let tokenizer = Tokenizer::new(IoReader::new("<title>hello world</title>".as_bytes()));
/// // more realistically: Tokenizer::new(File::open("index.html")?)
/// // long-form: Tokenizer::new(IoReader::new(File::open("index.html")?))
///
/// let mut new_html = String::new();
///
/// for token in tokenizer {
///     let token = token.unwrap();
///
///     match token {
///         Token::StartTag(tag) => {
///             write!(new_html, "<{}>", String::from_utf8_lossy(&tag.name)).unwrap();
///         }
///         Token::String(hello_world) => {
///             write!(new_html, "{}", String::from_utf8_lossy(&hello_world)).unwrap();
///         }
///         Token::EndTag(tag) => {
///             write!(new_html, "</{}>", String::from_utf8_lossy(&tag.name)).unwrap();
///         }
///         _ => panic!("unexpected input"),
///     }
///     
/// }
///
/// assert_eq!(new_html, "<title>hello world</title>");
/// ```
pub struct IoReader<R: Read> {
    buf: Box<[u8; BUF_SIZE]>,
    buf_offset: usize,
    buf_len: usize,
    reader: R,
}

const BUF_SIZE: usize = 16 * 1024;

impl<R: Read> IoReader<R> {
    /// Construct a new `BufReadReader` from any type that implements `Read`.
    pub fn new(reader: R) -> Self {
        IoReader {
            buf: Box::new([0; BUF_SIZE]),
            buf_offset: 0,
            buf_len: 0,
            reader,
        }
    }

    #[inline]
    fn prepare_buf(&mut self, min_len: usize) -> Result<(), io::Error> {
        // XXX: we don't do any utf8 validation here anymore
        debug_assert!(min_len < BUF_SIZE);
        let mut len = self.buf_len - self.buf_offset;
        if len < min_len {
            let mut raw_buf = &mut self.buf[..];
            raw_buf.rotate_left(self.buf_offset);
            raw_buf = &mut raw_buf[len..];
            while len < min_len {
                let n = self.reader.read(raw_buf)?;
                if n == 0 {
                    break;
                }
                len += n;
                raw_buf = &mut raw_buf[n..];
            }
            self.buf_len = len;
            self.buf_offset = 0;
        }
        Ok(())
    }
}

impl<R: Read> Reader for IoReader<R> {
    type Error = io::Error;

    fn read_byte(&mut self) -> Result<Option<u8>, Self::Error> {
        self.prepare_buf(1)?;
        if self.buf_offset == self.buf_len {
            return Ok(None);
        }
        let rv = self.buf.get(self.buf_offset).copied();
        if rv.is_some() {
            self.buf_offset += 1;
        }
        Ok(rv)
    }

    fn try_read_string(&mut self, s1: &[u8], case_sensitive: bool) -> Result<bool, Self::Error> {
        debug_assert!(!s1.contains(&b'\r'));
        debug_assert!(!s1.contains(&b'\n'));

        self.prepare_buf(s1.len())?;
        let s2 = &self.buf[self.buf_offset..min(self.buf_offset + s1.len(), self.buf_len)];
        if s1 == s2 || (!case_sensitive && s1.eq_ignore_ascii_case(s2)) {
            self.buf_offset += s1.len();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn read_until<'b>(
        &'b mut self,
        needle: &[u8],
        _: &'b mut [u8; 4],
    ) -> Result<Option<&'b [u8]>, Self::Error> {
        self.prepare_buf(4)?;
        let buf = &self.buf[self.buf_offset..self.buf_len];
        if !buf.is_empty() {
            if let Some(needle_pos) = fast_find(needle, buf) {
                if needle_pos == 0 {
                    self.buf_offset += 1;
                    Ok(Some(&buf[..1]))
                } else {
                    self.buf_offset += needle_pos;
                    Ok(Some(&buf[..needle_pos]))
                }
            } else {
                self.buf_offset += buf.len();
                Ok(Some(buf))
            }
        } else {
            Ok(None)
        }
    }
}

impl<'a> Readable<'a> for File {
    type Reader = IoReader<File>;

    fn to_reader(self) -> Self::Reader {
        IoReader::new(self)
    }
}

#[inline]
fn fast_find(needle: &[u8], haystack: &[u8]) -> Option<usize> {
    #[cfg(feature = "memchr")]
    if needle.iter().all(|x| x.is_ascii()) {
        if needle.len() == 3 {
            return memchr::memchr3(needle[0], needle[1], needle[2], haystack);
        } else if needle.len() == 2 {
            return memchr::memchr2(needle[0], needle[1], haystack);
        } else if needle.len() == 1 {
            return memchr::memchr(needle[0], haystack);
        }
    }

    let (i, _) = haystack
        .iter()
        .enumerate()
        .find(|(_, &b)| needle.contains(&b))?;
    Some(i)
}
