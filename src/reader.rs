/// An object that provides characters to the tokenizer.
///
/// See [`crate::Tokenizer::new`] for more information.
pub trait Reader {
    /// Return a new character from the input stream.
    ///
    /// Newlines have to be normalized as described in [Preprocessing the input
    /// stream](https://html.spec.whatwg.org/#preprocessing-the-input-stream), however error
    /// emission is done within the tokenizer.
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
    pos: usize,
}

impl<'a> StringReader<'a> {
    fn new(input: &'a str) -> Self {
        StringReader { input, pos: 0 }
    }

    fn peek_char(&self) -> Option<char> {
        self.input.get(self.pos..)?.chars().next()
    }
}

impl<'a> Reader for StringReader<'a> {
    fn read_char(&mut self) -> Option<char> {
        let mut r1 = match self.peek_char() {
            Some(x) => x,
            None => {
                self.pos += 1;
                return None;
            }
        };

        self.pos += r1.len_utf8();

        if r1 == '\r' {
            r1 = '\n';
            let r2 = self.peek_char();
            if r2 == Some('\n') {
                self.pos += r2.map(char::len_utf8).unwrap_or(0);
            }
        }

        Some(r1)
    }

    fn try_read_string(&mut self, s1: &str, case_sensitive: bool) -> bool {
        // we do not need to call validate_char here because `s` hopefully does not contain invalid
        // characters

        if let Some(s2) = self.input.get(self.pos..self.pos + s1.len()) {
            if s1 == s2 || (!case_sensitive && s1.eq_ignore_ascii_case(s2)) {
                self.pos += s1.len();
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
