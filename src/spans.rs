//! Source code spans.
//!
//! The [`DefaultEmitter`](crate::DefaultEmitter) is generic over a [`Span`].
//! This library comes with two Span implementations:
//!
//! * one for `()` which acts as the no-op implementation for when you don't want to track spans
//! * one for [`Range<usize>`] for when you do want to track spans
//!
//! To use the latter your reader however has to implement [`GetPos`].
//! You can easily use any existing reader by wrapping it in the [`PosTracker`] struct
//! which implements the [`GetPos`] trait and takes care of tracking the current position.

use std::ops::Range;

use crate::Reader;

/// A trait to be implemented by readers that track their own position.
pub trait GetPos {
    /// Returns the byte index of the current position.
    fn get_pos(&self) -> usize;
}

/// Wraps a [`Reader`] so that it implements [`GetPos`].
pub struct PosTracker<R> {
    /// The wrapped reader.
    pub reader: R,
    /// The current position.
    pub position: usize,
}

impl<R> GetPos for PosTracker<R> {
    fn get_pos(&self) -> usize {
        self.position
    }
}

/// Represents a character range in the source code.
pub trait Span<R>: Default + Clone {
    /// Initializes a new span at the current position of the reader.
    fn from_reader(reader: &R) -> Self;

    /// Initializes a new span at the current position of the reader with the given offset.
    fn from_reader_with_offset(reader: &R, offset: usize) -> Self;

    /// Extends the span by the length of the given string.
    fn push_str(&mut self, str: &str);
}

impl<R> Span<R> for () {
    fn from_reader(_reader: &R) -> Self {}

    fn from_reader_with_offset(_reader: &R, _offset: usize) -> Self {}

    fn push_str(&mut self, _str: &str) {}
}

impl<P: GetPos> Span<P> for Range<usize> {
    fn from_reader(reader: &P) -> Self {
        reader.get_pos() - 1..reader.get_pos() - 1
    }

    fn from_reader_with_offset(reader: &P, offset: usize) -> Self {
        reader.get_pos() - 1 + offset..reader.get_pos() - 1 + offset
    }

    fn push_str(&mut self, str: &str) {
        self.end += str.len();
    }
}

impl<R: Reader> Reader for PosTracker<R> {
    type Error = R::Error;

    fn read_char(&mut self) -> Result<Option<char>, Self::Error> {
        match self.reader.read_char()? {
            Some(char) => {
                self.position += char.len_utf8();
                Ok(Some(char))
            }
            None => Ok(None),
        }
    }

    fn try_read_string(&mut self, s: &str, case_sensitive: bool) -> Result<bool, Self::Error> {
        match self.reader.try_read_string(s, case_sensitive)? {
            true => {
                self.position += s.len();
                Ok(true)
            }
            false => Ok(false),
        }
    }
}
