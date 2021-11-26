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
        debug_assert!(!s1.contains("\r"));
        debug_assert!(!s1.contains("\n"));

        if let Some(s2) = self.get_remaining_line()?.get(..s1.len()) {
            if s1 == s2 || (!case_sensitive && s1.eq_ignore_ascii_case(s2)) {
                self.line_pos += s1.len();
                return Ok(true);
            }
        }

        Ok(false)
    }
}

impl<'a, R: Read + 'a> Readable<'a> for BufReader<R> {
    type Reader = BufReadReader<BufReader<R>>;

    fn to_reader(self) -> Self::Reader {
        BufReadReader::new(self)
    }
}
