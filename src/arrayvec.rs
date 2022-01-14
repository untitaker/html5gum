/// This is basically like the arrayvec crate, except crappier, only the subset I need and
/// therefore without unsafe Rust.

pub struct ArrayVec<T: Copy, const CAP: usize> {
    content: [T; CAP],
    len: usize,
}

impl<T: Copy, const CAP: usize> ArrayVec<T, CAP> {
    pub fn new(filler_item: T) -> Self {
        // filler_item is there to avoid usage of MaybeUninit, and can literally be anything at
        // all.
        ArrayVec {
            content: [filler_item; CAP],
            len: 0,
        }
    }

    pub fn push(&mut self, item: T) {
        self.content[self.len] = item;
        self.len += 1;
    }

    pub fn drain(&mut self) -> &[T] {
        let rv = &self.content[..self.len];
        self.len = 0;
        rv
    }
}
