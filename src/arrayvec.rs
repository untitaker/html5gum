/// Similar to [`arrayvec::ArrayVec`], but only has limited capabilities that we need.
///
/// [`arrayvec::ArrayVec`]: https://docs.rs/arrayvec/latest/arrayvec/struct.ArrayVec.html
pub(crate) struct ArrayVec<T, const CAP: usize>(arrayvec::ArrayVec<T, CAP>);

impl<T, const CAP: usize> ArrayVec<T, CAP> {
    pub(crate) fn new() -> Self {
        Self(arrayvec::ArrayVec::new())
    }

    pub(crate) fn push(&mut self, element: T) {
        self.0.push(element);
    }

    pub(crate) fn drain(&mut self) -> arrayvec::Drain<T, CAP> {
        self.0.drain(0..self.0.len())
    }
}
