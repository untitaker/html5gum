use std::error;
use std::fmt;

/// Definition of an empty enum.
///
/// This is used as the error type in situations where there can't be an error. A `Result<T, Never>`
/// can be safely unwrapped and the `unwrap()` may be optimized away entirely.
///
/// This error is typically encountered when attempting to get tokens from the `Tokenizer`. Call
/// [`crate::Tokenizer::infallible`] if you wish to avoid unwrapping those results yourself.
pub enum Never {}

impl fmt::Display for Never {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        match *self {}
    }
}
impl fmt::Debug for Never {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        match *self {}
    }
}

impl error::Error for Never {}
