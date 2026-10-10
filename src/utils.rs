macro_rules! surrogate_pat {
    () => {
        0xd800..=0xdfff
    };
}

pub(crate) use surrogate_pat;

macro_rules! noncharacter_pat {
    () => {
        0xfdd0
            ..=0xfdef
                | 0xfffe
                | 0xffff
                | 0x1fffe
                | 0x1ffff
                | 0x2fffe
                | 0x2ffff
                | 0x3fffe
                | 0x3ffff
                | 0x4fffe
                | 0x4ffff
                | 0x5fffe
                | 0x5ffff
                | 0x6fffe
                | 0x6ffff
                | 0x7fffe
                | 0x7ffff
                | 0x8fffe
                | 0x8ffff
                | 0x9fffe
                | 0x9ffff
                | 0xafffe
                | 0xaffff
                | 0xbfffe
                | 0xbffff
                | 0xcfffe
                | 0xcffff
                | 0xdfffe
                | 0xdffff
                | 0xefffe
                | 0xeffff
                | 0xffffe
                | 0xfffff
                | 0x10fffe
                | 0x10ffff
    };
}

pub(crate) use noncharacter_pat;

macro_rules! ctostr {
    ($c:expr) => {
        &*$c.encode_utf8(&mut [0; 4])
    };
}

pub(crate) use ctostr;

/// Size of the stack buffer used by [`with_lowercase_str`]. Large enough for virtually all tag and
/// attribute names, longer ones are passed to the callback in multiple chunks.
const LOWERCASE_BUF_LEN: usize = 64;

/// Repeatedly call `f` with chunks of lowercased characters from `s`.
///
/// Only ASCII uppercase letters are changed. Chunks are not guaranteed to end on a UTF-8 character
/// boundary.
// Out of line, every call, including the far more common all-lowercase case, would pay for the
// larger prologue of the buffer path.
#[inline(always)]
pub(crate) fn with_lowercase_str(s: &[u8], mut f: impl FnMut(&[u8])) {
    if s.iter().any(u8::is_ascii_uppercase) {
        // Lowercase into a stack buffer so that `f` is called once per buffer-full rather than
        // once per byte.
        let mut buf = [0; LOWERCASE_BUF_LEN];
        for chunk in s.chunks(LOWERCASE_BUF_LEN) {
            for (dst, src) in buf.iter_mut().zip(chunk) {
                *dst = src.to_ascii_lowercase();
            }
            f(&buf[..chunk.len()]);
        }
    } else {
        f(s);
    }
}

#[test]
fn test_with_lowercase_str() {
    fn chunks(s: &[u8]) -> Vec<Vec<u8>> {
        let mut rv = Vec::new();
        with_lowercase_str(s, |chunk| rv.push(chunk.to_vec()));
        rv
    }

    assert_eq!(chunks(b"viewbox"), [b"viewbox"]);
    assert_eq!(chunks(b"viewBox"), [b"viewbox"]);

    // Every byte value: only ASCII uppercase letters change, non-ASCII bytes are left alone.
    let all_bytes: Vec<u8> = (0..=255).collect();
    assert_eq!(chunks(&all_bytes).concat(), all_bytes.to_ascii_lowercase());

    // Longer than the buffer, with multi-byte characters crossing chunk boundaries.
    let long = "B\u{c4}\u{20ac}".repeat(LOWERCASE_BUF_LEN + 1);
    let long_chunks = chunks(long.as_bytes());
    assert_eq!(
        long_chunks.len(),
        long.len().div_ceil(LOWERCASE_BUF_LEN),
        "one call per buffer-full"
    );
    assert_eq!(
        long_chunks.concat(),
        "b\u{c4}\u{20ac}".repeat(LOWERCASE_BUF_LEN + 1).as_bytes()
    );
}

// having this be a macro is performance critical. rustc appears to be unable to optimize away code
// like this:
//
// ```rust
// fn noop(s: &str) {}
//
// noop(&format!("foo"));
// ```
//
// format!() + its string allocation still exists in resulting code
macro_rules! trace_log {
    ($($tt:tt)*) => {{
        #[cfg(debug_assertions)]
        crate::testutils::trace_log(&format!($($tt)*));
    }};
}

pub(crate) use trace_log;
