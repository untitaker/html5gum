use std::cmp::min;
use std::convert::Infallible;
use std::fmt::Debug;
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
    /// `read_until` if there is any sort of in-memory buffer where some sort of efficient string
    /// search (see `memchr` or `jetscii` crate) can be run on.
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
        let _ = needle;

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
#[derive(Debug)]
pub struct StringReader<'a> {
    input: &'a [u8],
}

impl<'a> StringReader<'a> {
    fn new(input: &'a [u8]) -> Self {
        StringReader { input }
    }
}

impl<'a> Reader for StringReader<'a> {
    type Error = Infallible;

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
#[derive(Debug)]
pub struct IoReader<R: Read, Buffer: AsMut<[u8]> = Box<[u8]>> {
    buf: Buffer,
    read_cursor: usize,
    write_cursor: usize,
    reader: R,
}

impl<R: Read> IoReader<R> {
    /// Construct a new `BufReadReader` from any type that implements `Read`.
    pub fn new(reader: R) -> Self {
        Self::new_with_buffer_size::<16384>(reader)
    }

    /// Construct a new `BufReadReader` with a specific internal buffer size.
    ///
    /// `new` defaults to a heap-allocated buffer of size 16kB.
    pub fn new_with_buffer_size<const BUF_SIZE: usize>(reader: R) -> Self {
        Self::new_with_buffer_impl(reader, Box::new([0; BUF_SIZE]))
    }
}

impl<'a, R: Read> IoReader<R, &'a mut [u8]> {
    /// Instantiate IoReader with a custom kind of buffer.
    ///
    /// Buffers do not need to be zero-initialized.
    pub fn new_with_buffer(reader: R, buf: &'a mut [u8]) -> Self {
        Self::new_with_buffer_impl(reader, buf)
    }
}

impl<R: Read, Buffer: AsMut<[u8]>> IoReader<R, Buffer> {
    // new_with_buffer_impl is not exposed because we cannot use any kind of AsMut. It has to be
    // one where we can be sure that the size of the buffer does not change with repeated calls to
    // `as_mut()`. There are complex solutions to this sort of thing, but for now it seems simpler
    // to allow either Box<[u8; _]> or &mut [u8], and nothing else.
    //
    // See discussion at https://users.rust-lang.org/t/cowmut-or-borrowed-owned-mutable-temp-buffers/96595
    fn new_with_buffer_impl(reader: R, buf: Buffer) -> Self {
        IoReader {
            buf,
            read_cursor: 0,
            write_cursor: 0,
            reader,
        }
    }

    /// Ensure that the buffer contains at leaast `min_read_len` bytes to read.
    ///
    /// Shift all to-be-read buffer contents between `self.read_cursor` and `self.write_cursor` to
    /// the beginning of the buffer, and read extra bytes if necessary.
    fn prepare_buf(&mut self, min_read_len: usize) -> Result<(), io::Error> {
        let mut readable_len = self.write_cursor - self.read_cursor;
        debug_assert!(min_read_len <= self.buf.as_mut().len());
        debug_assert!(readable_len <= self.buf.as_mut().len());
        if readable_len < min_read_len {
            let mut raw_buf = &mut self.buf.as_mut()[..];
            raw_buf.copy_within(self.read_cursor..self.write_cursor, 0);
            raw_buf = &mut raw_buf[readable_len..];
            while readable_len < min_read_len {
                let n = self.reader.read(raw_buf)?;
                if n == 0 {
                    break;
                }
                readable_len += n;
                raw_buf = &mut raw_buf[n..];
            }
            self.write_cursor = readable_len;
            self.read_cursor = 0;
        }
        Ok(())
    }
}

impl<R: Read, Buffer: AsMut<[u8]>> Reader for IoReader<R, Buffer> {
    type Error = io::Error;

    fn read_byte(&mut self) -> Result<Option<u8>, Self::Error> {
        self.prepare_buf(1)?;
        if self.read_cursor == self.write_cursor {
            return Ok(None);
        }
        let rv = self.buf.as_mut().get(self.read_cursor).copied();
        if rv.is_some() {
            self.read_cursor += 1;
        }
        Ok(rv)
    }

    fn try_read_string(&mut self, s1: &[u8], case_sensitive: bool) -> Result<bool, Self::Error> {
        debug_assert!(!s1.contains(&b'\r'));
        debug_assert!(!s1.contains(&b'\n'));

        self.prepare_buf(s1.len())?;
        let s2 = &self.buf.as_mut()
            [self.read_cursor..min(self.read_cursor + s1.len(), self.write_cursor)];
        if s1 == s2 || (!case_sensitive && s1.eq_ignore_ascii_case(s2)) {
            self.read_cursor += s1.len();
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
        let buf = &self.buf.as_mut()[self.read_cursor..self.write_cursor];
        if buf.is_empty() {
            Ok(None)
        } else if let Some(needle_pos) = fast_find(needle, buf) {
            if needle_pos == 0 {
                self.read_cursor += 1;
                Ok(Some(&buf[..1]))
            } else {
                self.read_cursor += needle_pos;
                Ok(Some(&buf[..needle_pos]))
            }
        } else {
            self.read_cursor += buf.len();
            Ok(Some(buf))
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
    #[cfg(feature = "jetscii")]
    {
        debug_assert!(needle.len() <= 16);
        let mut needle_arr = [0; 16];
        needle_arr[..needle.len()].copy_from_slice(needle);
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        jetscii::Bytes::new(needle_arr, needle.len() as i32, |b| needle.contains(&b)).find(haystack)
    }

    #[cfg(not(feature = "jetscii"))]
    haystack.iter().position(|b| needle.contains(b))
}
