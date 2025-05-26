use crate::{reader::SpanReader, Reader};

/// A single bound of a [`Span`].
///
/// For example use `()` as a bound to ignore spans and use [`usize`] for the default
/// implementation which implements [`SpanBoundFromReader`] for [`SpanReader`].
pub trait SpanBound:
    Sized
    + Clone
    + Copy
    + std::fmt::Debug
    + Default
    + PartialEq
    + Eq
    + PartialOrd
    + Ord
    + std::hash::Hash
{
    /// Offset the bound by a given value.
    #[must_use]
    fn offset(self, by: isize) -> Self;
}

/// A [`SpanBound`] which can be read from the given [`Reader`].
pub trait SpanBoundFromReader<R>: SpanBound {
    /// Get the current position from the reader.
    ///
    /// For example, [`SpanReader`] tracks the current byte position in the input stream.
    #[must_use]
    fn from_reader(reader: &R) -> Self;

    /// Shortcut for `Self::from_reader(reader).offset(-1)`.
    #[inline]
    fn from_reader_previous(reader: &R) -> Self {
        Self::from_reader(reader).offset(-1)
    }
}

/// Position/ boundary `start..end` in the input.
///
/// The position can be a byte, character offset or something else, depending on the [`Reader`] it
/// originates from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Span<B: SpanBound = usize> {
    /// Start position (inclusive) of the span.
    pub start: B,
    /// End position (exclusive) of the span.
    pub end: B,
}

impl<S: SpanBound> Span<S> {
    #[must_use]
    pub(crate) fn empty_at<R: Reader>(reader: &R) -> Self
    where
        S: SpanBoundFromReader<R>,
    {
        let v = S::from_reader(reader);
        Self { start: v, end: v }
    }
}

impl Span<()> {
    /// Dummy empty span for tests.
    #[doc(hidden)]
    pub const DUMMY: Self = Self { start: (), end: () };
}

impl SpanBound for () {
    fn offset(self, _by: isize) -> Self {}
}

impl<R: Reader> SpanBoundFromReader<R> for () {
    fn from_reader(_reader: &R) -> Self {}
    fn from_reader_previous(_reader: &R) -> Self {}
}

impl SpanBound for usize {
    fn offset(self, by: isize) -> Self {
        self.saturating_add_signed(by)
    }
}

impl<R: Reader> SpanBoundFromReader<SpanReader<R>> for usize {
    fn from_reader(reader: &SpanReader<R>) -> Self {
        reader.position
    }

    fn from_reader_previous(reader: &SpanReader<R>) -> Self {
        reader.position.saturating_sub(1)
    }
}

/// A value together with its [`Span`].
#[allow(missing_docs)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Spanned<T, B: SpanBound = usize> {
    pub value: T,
    pub span: Span<B>,
}

impl<T, B: SpanBound> std::ops::Deref for Spanned<T, B> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<T, B: SpanBound> std::ops::DerefMut for Spanned<T, B> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}
