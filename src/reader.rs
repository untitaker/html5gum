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
    #[inline(always)]
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
/// for Ok(token) in Tokenizer::new(html) {
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

    #[inline(always)]
    fn read_byte(&mut self) -> Result<Option<u8>, Self::Error> {
        if self.input.is_empty() {
            Ok(None)
        } else {
            let rv = self.input[0];
            self.input = &self.input[1..];
            Ok(Some(rv))
        }
    }

    #[inline(always)]
    fn read_until<'b>(
        &'b mut self,
        needle: &[u8],
        _: &'b mut [u8; 4],
    ) -> Result<Option<&'b [u8]>, Self::Error> {
        if self.input.is_empty() {
            return Ok(None);
        }

        let (rv, rest) = split_chunk(needle, self.input);
        self.input = rest;
        Ok(Some(rv))
    }

    #[inline(always)]
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
pub struct IoReader<R: Read, Buffer: AsRef<[u8]> + AsMut<[u8]> = Box<[u8]>> {
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
    /// Instantiate `IoReader` with a custom kind of buffer.
    ///
    /// Buffers do not need to be zero-initialized.
    pub fn new_with_buffer(reader: R, buf: &'a mut [u8]) -> Self {
        Self::new_with_buffer_impl(reader, buf)
    }
}

impl<R: Read, Buffer: AsRef<[u8]> + AsMut<[u8]>> IoReader<R, Buffer> {
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
    #[inline(always)]
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

impl<R: Read, Buffer: AsRef<[u8]> + AsMut<[u8]>> Reader for IoReader<R, Buffer> {
    type Error = io::Error;

    #[inline(always)]
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

    #[inline(always)]
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

    #[inline(always)]
    fn read_until<'b>(
        &'b mut self,
        needle: &[u8],
        _: &'b mut [u8; 4],
    ) -> Result<Option<&'b [u8]>, Self::Error> {
        self.prepare_buf(4)?;
        let buf = &self.buf.as_ref()[self.read_cursor..self.write_cursor];
        if buf.is_empty() {
            Ok(None)
        } else {
            let (rv, _) = split_chunk(needle, buf);
            self.read_cursor += rv.len();
            Ok(Some(rv))
        }
    }
}

impl<'a> Readable<'a> for File {
    type Reader = IoReader<File>;

    fn to_reader(self) -> Self::Reader {
        IoReader::new(self)
    }
}

/// Split `haystack` into the chunk that `read_until` returns and the rest: the first byte alone if
/// it is in `needle`, otherwise everything before the first byte that is in `needle` (the whole
/// haystack if there is none).
///
/// The split has to be exact, and not just somewhere before the first needle byte: chunk
/// boundaries are observable, for example in the spans of character errors.
#[inline(always)]
fn split_chunk<'a>(needle: &[u8], haystack: &'a [u8]) -> (&'a [u8], &'a [u8]) {
    #[cfg(feature = "jetscii")]
    {
        debug_assert!(needle.len() <= 16);
        let mut needle_arr = [0; 16];
        needle_arr[..needle.len()].copy_from_slice(needle);
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let pos = jetscii::Bytes::new(needle_arr, needle.len() as i32, |b| needle.contains(&b))
            .find(haystack);
        match pos {
            Some(pos) => haystack.split_at(pos.max(1)),
            None => (haystack, &[]),
        }
    }

    #[cfg(not(feature = "jetscii"))]
    swar_split_chunk(needle, haystack)
}

/// Portable "SIMD within a register" implementation of `split_chunk`, processing 8 bytes at a
/// time.
///
/// The search runs a list of tests on every 8-byte word `x`. A test `(m, c, n)` flags the bytes `b`
/// with `(b & m) ^ c < n`: with `y = (x & splat(m)) ^ splat(c)`, that is
/// `y.wrapping_sub(splat(n)) & !y & HI` (for `n <= 0x80`). That formula can produce false
/// positives, but only in bytes *above* a truly flagged byte (they are caused by the borrow out of
/// that byte), so the lowest flagged byte is always exact. OR-ing the results of all tests keeps
/// that property: the lowest flagged byte of the union is the lowest of the per-test lowest flagged
/// bytes, each of which is exact. Words are loaded little-endian so that the lowest byte is the
/// first one in memory.
///
/// A needle byte `b` can always be tested on its own with `(0xff, b, 1)`. To need fewer tests per
/// word, some groups of needle bytes are tested together, but the combined tests also flag some
/// bytes that are not in `needle`. So the lowest flagged byte is verified, and if it is not in
/// `needle`, the rest of the haystack is searched with one exact test per needle byte.
#[inline(always)]
#[cfg(any(test, not(feature = "jetscii")))]
fn swar_split_chunk<'a>(needle: &[u8], haystack: &'a [u8]) -> (&'a [u8], &'a [u8]) {
    // Very often the delimiter is the very first byte, so check that cheaply before doing any word
    // setup.
    match haystack.first() {
        None => return (haystack, haystack),
        Some(first) if needle.contains(first) => return haystack.split_at(1),
        Some(_) => {}
    }

    // This only depends on `needle`, which is a constant after inlining, so the tests are computed
    // at compile time.
    let mut tests = [(0xff, 0, 1); 16];
    let mut num_tests = 0;
    // States that stop at all whitespace test for `b < 0x21` instead of testing whitespace, NUL and
    // CR individually. This also flags other control characters, which are rare.
    let low = needle.contains(&b'\t')
        && needle.contains(&b'\n')
        && needle.contains(&b'\x0c')
        && needle.contains(&b' ');
    if low {
        tests[num_tests] = (0xff, 0, 0x21);
        num_tests += 1;
    }
    // Most states stop at NUL and CR (which are rare), but not at tab or LF (which may be
    // frequent). `b & 0xf2 == 0` flags NUL and CR, but also 0x01, 0x04, 0x05, 0x08, tab and 0x0c,
    // and no other byte.
    let nul_cr = !low && needle.contains(&0) && needle.contains(&b'\r');
    if nul_cr {
        tests[num_tests] = (0xf2, 0, 1);
        num_tests += 1;
    }
    for &b in needle {
        if !(low && b < 0x21) && !(nul_cr && (b == 0 || b == b'\r')) {
            tests[num_tests] = (0xff, b, 1);
            num_tests += 1;
        }
    }

    // The first byte is known not to be in `needle`, but words are cheaper to load from offset 0.
    match swar_scan(needle, &tests[..num_tests], haystack, 0, low || nul_cr) {
        Ok(rv) => rv,
        Err(false_candidate) => {
            let mut exact = [(0xff, 0, 1); 16];
            for (test, &b) in exact.iter_mut().zip(needle) {
                test.1 = b;
            }
            match swar_scan(
                needle,
                &exact[..needle.len()],
                haystack,
                false_candidate + 1,
                false,
            ) {
                Ok(rv) => rv,
                // Exact tests do not produce false candidates.
                Err(_) => unreachable!(),
            }
        }
    }
}

/// Run the tests on one word, see `swar_split_chunk`.
#[inline(always)]
#[cfg(any(test, not(feature = "jetscii")))]
fn swar_word(tests: &[(u8, u8, u8)], chunk: &[u8; 8]) -> u64 {
    const LO: u64 = u64::from_ne_bytes([0x01; 8]);
    const HI: u64 = u64::from_ne_bytes([0x80; 8]);

    let x = u64::from_le_bytes(*chunk);
    let mut found = 0;
    for &(m, c, n) in tests {
        let y = (x & (LO * u64::from(m))) ^ (LO * u64::from(c));
        found |= y.wrapping_sub(LO * u64::from(n)) & !y & HI;
    }
    found
}

/// Search `haystack[offset..]` with the given tests and split `haystack` at the first flagged byte,
/// see `swar_split_chunk`. If `verify` is set and the lowest flagged byte is not actually in
/// `needle`, its position is returned as `Err`.
#[inline(always)]
#[cfg(any(test, not(feature = "jetscii")))]
fn swar_scan<'a>(
    needle: &[u8],
    tests: &[(u8, u8, u8)],
    haystack: &'a [u8],
    mut offset: usize,
    verify: bool,
) -> Result<(&'a [u8], &'a [u8]), usize> {
    // Most chunks are short, so check one word before checking two at a time.
    if let Some(chunk) = haystack.get(offset..).and_then(|rest| rest.first_chunk()) {
        let found = swar_word(tests, chunk);
        if found != 0 {
            let pos = offset + (found.trailing_zeros() / 8) as usize;
            if verify && !needle.contains(&haystack[pos]) {
                return Err(pos);
            }
            return Ok(haystack.split_at(pos));
        }
        offset += 8;
    }

    let mut chunks = haystack[offset..].chunks_exact(16);
    for chunk in &mut chunks {
        let found0 = swar_word(tests, chunk.first_chunk().unwrap());
        let found1 = swar_word(tests, chunk.last_chunk().unwrap());
        if found0 | found1 != 0 {
            let pos = offset
                + if found0 != 0 {
                    (found0.trailing_zeros() / 8) as usize
                } else {
                    8 + (found1.trailing_zeros() / 8) as usize
                };
            if verify && !needle.contains(&haystack[pos]) {
                return Err(pos);
            }
            return Ok(haystack.split_at(pos));
        }
        offset += 16;
    }

    match chunks.remainder().iter().position(|b| needle.contains(b)) {
        Some(i) => Ok(haystack.split_at(offset + i)),
        None => Ok((haystack, &[])),
    }
}

#[cfg(test)]
mod tests {
    use super::{split_chunk, swar_split_chunk};
    use proptest::prelude::*;

    /// The needles used by the tokenizer, plus some that provoke SWAR edge cases.
    const NEEDLES: &[&[u8]] = &[
        b"\r",
        b"<\r",
        b"&<\r",
        b"\0\r",
        b"\x01\r",
        b"\x80\r",
        b"\xff\x7f\r",
        b"\t\n\x0c />\0\r",
        b"\t\n\x0c \"&'<=>`\0\r",
        b"\"&\0\r",
        b"&<\0\r",
        b"\t\n\x0c \r",
        b"\t\n\x0c >\0\r",
        b"\t\n\x0c />=\0\"'<\r",
    ];

    fn naive_split_chunk<'a>(needle: &[u8], haystack: &'a [u8]) -> (&'a [u8], &'a [u8]) {
        match haystack.iter().position(|b| needle.contains(b)) {
            Some(pos) => haystack.split_at(pos.max(1)),
            None => (haystack, &[]),
        }
    }

    fn check(needle: &[u8], haystack: &[u8]) {
        let expected = naive_split_chunk(needle, haystack);
        assert_eq!(
            swar_split_chunk(needle, haystack),
            expected,
            "swar_split_chunk needle={needle:?} haystack={haystack:?}"
        );
        assert_eq!(
            split_chunk(needle, haystack),
            expected,
            "split_chunk needle={needle:?} haystack={haystack:?}"
        );
    }

    #[test]
    fn needle_at_every_offset() {
        // Filler bytes chosen to provoke SWAR edge cases: bytes adjacent to needle bytes (0x01 and
        // 0x81 next to 0x00 and 0x80), high-bit bytes, and bytes that differ by one from a needle.
        let fillers: &[u8] = &[
            b'a', 0x00, 0x01, 0x02, 0x04, 0x05, 0x08, b'\t', 0x0c, 0x0e, 0x1f, b' ', b'!', 0x7f,
            0x80, 0x81, 0x8d, 0xa1, 0xfe, 0xff,
        ];

        for needle in NEEDLES {
            for len in 0..60 {
                for &filler in fillers {
                    let haystack = vec![filler; len];
                    check(needle, &haystack);
                    for pos in 0..len {
                        for &hit in *needle {
                            let mut h = haystack.clone();
                            h[pos] = hit;
                            check(needle, &h);
                            if pos + 1 < len {
                                // a byte next to a needle byte, then also a second hit later
                                h[pos + 1] = 0x01;
                                check(needle, &h);
                                h[len - 1] = hit;
                                check(needle, &h);
                            }
                        }
                    }
                }
            }
        }
    }

    /// A needle, and a haystack biased towards needle bytes, their neighbours and control
    /// characters (which the grouped tests in `swar_split_chunk` flag as candidates).
    fn needle_and_haystack() -> impl Strategy<Value = (Vec<u8>, Vec<u8>)> {
        prop_oneof![
            prop::sample::select(NEEDLES).prop_map(<[u8]>::to_vec),
            prop::collection::vec(any::<u8>(), 1..=13),
        ]
        .prop_flat_map(|needle| {
            let byte = prop_oneof![
                prop::sample::select(needle.clone()),
                prop::sample::select(needle.clone()).prop_map(|b| b.wrapping_add(1)),
                prop::sample::select(needle.clone()).prop_map(|b| b ^ 0x80),
                0u8..0x23,
                any::<u8>(),
            ];
            (Just(needle), prop::collection::vec(byte, 0..80))
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(10_000))]

        #[test]
        fn matches_naive_find((needle, haystack) in needle_and_haystack()) {
            let expected = naive_split_chunk(&needle, &haystack);
            prop_assert_eq!(swar_split_chunk(&needle, &haystack), expected);
            prop_assert_eq!(split_chunk(&needle, &haystack), expected);
        }
    }
}
