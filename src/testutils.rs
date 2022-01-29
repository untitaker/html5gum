//! Module of helper functions for integration tests.
//!
//! Those tests should only test public API surface in general, with some exceptions as provided by
//! this module.
use crate::Reader;
use std::cell::Cell;

pub use crate::utils::State;

thread_local! {
    /// Buffer of all debugging output logged internally by html5gum.
    pub static OUTPUT: Cell<String> = Cell::default();
}

/// Simple debug logger for tests.
///
/// The test harness used by `tests/html5lib_tokenizer.rs` cannot capture stdout, see [libtest-mimic
/// issue #9](https://github.com/LukasKalbertodt/libtest-mimic/issues/9) -- this is much more performant
/// than println anyway though.
///
/// A noop version for non-test builds is implemented in src/lib.rs
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
