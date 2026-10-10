//! Module of helper functions for integration tests.
//!
//! Those tests should only test public API surface in general, with some exceptions as provided by
//! this module.
use crate::Reader;
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

/// Global flag to enable trace logging. When false, the `trace_log!` macro neither formats nor
/// logs its message.
///
/// During fuzzing, we want to run with cfg(debug_assertions) yet at the same time not run
/// trace_log to keep target stability (in AFL terms). Adding more cargo featureflags is unwieldy.
static TRACE_LOG_ENABLED: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// Buffer of all debugging output logged internally by html5gum.
    pub static OUTPUT: Cell<String> = Cell::default();
}

/// Enable trace logging for the current process.
/// This should be called at the start of any test that needs to capture trace logs.
pub fn enable_trace_log() {
    TRACE_LOG_ENABLED.store(true, Ordering::Relaxed);
}

/// Disable trace logging for the current process.
pub fn disable_trace_log() {
    TRACE_LOG_ENABLED.store(false, Ordering::Relaxed);
}

/// Whether trace logging is enabled. Checked by the `trace_log!` macro before formatting.
#[inline]
pub fn trace_log_enabled() -> bool {
    TRACE_LOG_ENABLED.load(Ordering::Relaxed)
}

/// Simple debug logger for tests.
///
/// The test harness used by `tests/html5lib_tokenizer.rs` cannot capture stdout, see [libtest-mimic
/// issue #9](https://github.com/LukasKalbertodt/libtest-mimic/issues/9) -- this is much more performant
/// than println anyway though.
///
/// Crate code logs through the `trace_log!` macro in src/utils.rs instead, which compiles to
/// nothing without debug_assertions and only formats its message if trace logging is enabled. This
/// function itself logs unconditionally.
pub fn trace_log(msg: &str) {
    OUTPUT.with(|cell| {
        let mut buf = cell.take();
        buf.push_str(msg);
        buf.push('\n');

        if buf.len() > 20 * 1024 * 1024 {
            buf.clear();
            buf.push_str("[truncated output]\n");
        }

        cell.set(buf);
    });
}

/// All named character references in the generated table, as pairs of the name (without the
/// leading `&`) and the characters it stands for.
pub fn named_character_references() -> Vec<(String, &'static str)> {
    let mut rv = Vec::new();

    for first_char in 0..=u8::MAX {
        for second_char in 0..=u8::MAX {
            for &(name, characters) in crate::entities::candidates(first_char, second_char) {
                assert_eq!(name.as_bytes()[0], second_char);
                rv.push((format!("{}{}", char::from(first_char), name), characters));
            }
        }
    }

    rv
}

/// A kind of reader that implements `read_until` very poorly. Only available in tests
#[derive(Debug)]
pub struct SlowReader<R: Reader>(pub R);

impl<R: Reader> Reader for SlowReader<R> {
    type Error = R::Error;

    fn read_byte(&mut self) -> Result<Option<u8>, Self::Error> {
        self.0.read_byte()
    }

    fn try_read_string(&mut self, s: &[u8], case_sensitive: bool) -> Result<bool, Self::Error> {
        self.0.try_read_string(s, case_sensitive)
    }
}
