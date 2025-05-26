/// A single bound of a [`Span`].
///
/// For example use `()` as a bound to ignore spans and use [`usize`] for the default
/// implementation.
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

/// Position/ boundary `start..end` in the input.
///
/// The position will mostly be a byte offset, but depending on the [crate::Reader] it originates
/// from, it can be something entirely else.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Span<B: SpanBound = usize> {
    /// Start position (inclusive) of the span.
    pub start: B,
    /// End position (exclusive) of the span.
    pub end: B,
}

impl Span<()> {
    /// Dummy empty span for tests.
    #[doc(hidden)]
    pub const DUMMY: Self = Self { start: (), end: () };
}

impl SpanBound for () {
    fn offset(self, _by: isize) -> Self {}
}

impl SpanBound for usize {
    fn offset(self, by: isize) -> Self {
        self.saturating_add_signed(by)
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
