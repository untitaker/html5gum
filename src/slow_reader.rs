use crate::Reader;

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
