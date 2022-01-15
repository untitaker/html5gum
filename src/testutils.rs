//! Module of helper functions for integration tests.
//!
//! Those tests should only test public API surface in general, with some exceptions as provided by
//! this module.
use crate::Reader;
use std::sync::Mutex;

pub use crate::utils::State;

thread_local! {
    /// Buffer of all debugging output logged internally by html5gum.
    pub static OUTPUT: Mutex<String> = Default::default();
}

/// Simple debug logger for tests.
///
/// The test harness used by tests/html5lib_tokenizer.rs cannot capture stdout, see
/// https://github.com/LukasKalbertodt/libtest-mimic/issues/9 -- this is much more performant
/// than println anyway though.
///
/// A noop version for non-test builds is implemented in src/lib.rs
pub fn trace_log(msg: String) {
    OUTPUT.with(|lock| {
        let mut buf = lock.lock().unwrap();
        buf.push_str(&msg);
        buf.push_str('\n');

        if buf.len() > 20 * 1024 * 1024 {
            buf.clear();
            buf.push_str("[truncated output]\n");
        }
    });
}

/// A kind of reader that implements read_until very poorly. Only available in tests
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
