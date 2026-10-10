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

/// Repeatedly call `f` with chunks of lowercased characters from `s`.
pub(crate) fn with_lowercase_str(s: &[u8], mut f: impl FnMut(&[u8])) {
    if s.iter().any(u8::is_ascii_uppercase) {
        for x in s {
            f(&[x.to_ascii_lowercase()]);
        }
    } else {
        f(s);
    }
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
//
// For the same reason the runtime flag has to be checked here, before the arguments are evaluated
// and formatted: this code is compiled into every build with debug_assertions, including those of
// downstream crates (dev profile, fuzzing), and some messages format entire chunks of input.
macro_rules! trace_log {
    ($($tt:tt)*) => {{
        #[cfg(debug_assertions)]
        if crate::testutils::trace_log_enabled() {
            crate::testutils::trace_log(&format!($($tt)*));
        }
    }};
}

pub(crate) use trace_log;

#[cfg(debug_assertions)]
#[test]
fn trace_log_formats_only_when_enabled() {
    use std::cell::Cell;
    use std::fmt;

    struct CountFormats<'a>(&'a Cell<usize>);

    impl fmt::Debug for CountFormats<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            self.0.set(self.0.get() + 1);
            f.write_str("CountFormats")
        }
    }

    let formats = Cell::new(0);

    // Trace logging is off by default, and no other unit test turns it on.
    trace_log!("{:?}", CountFormats(&formats));
    assert_eq!(formats.get(), 0);

    crate::testutils::enable_trace_log();
    trace_log!("{:?}", CountFormats(&formats));
    crate::testutils::disable_trace_log();
    assert_eq!(formats.get(), 1);
}
