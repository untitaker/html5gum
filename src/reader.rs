/// An object that provides characters to the tokenizer.
///
/// See [`crate::Tokenizer::new`] for more information.
pub trait Reader {
    /// Return a new character from the input stream.
    ///
    /// The input stream does **not** have to be preprocessed in any way, it can contain standalone
    /// surrogates and have inconsistent newlines.
    fn read_char(&mut self) -> Option<char>;

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
    fn try_read_string(&mut self, s: &str, case_sensitive: bool) -> bool;
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
    fn read_char(&mut self) -> Option<char> {
        let c = self.cursor.next()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn try_read_string(&mut self, s1: &str, case_sensitive: bool) -> bool {
        // we do not need to call validate_char here because `s` hopefully does not contain invalid
        // characters
        if let Some(s2) = self.input.get(self.pos..self.pos + s1.len()) {
            if s1 == s2 || (!case_sensitive && s1.eq_ignore_ascii_case(s2)) {
                self.pos += s1.len();
                self.cursor = self.input[self.pos..].chars();
                return true;
            }
        }

        false
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
