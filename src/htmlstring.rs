use std::borrow::{Borrow, BorrowMut};
use std::fmt::{Debug, Formatter};
use std::ops::{Deref, DerefMut};

/// A wrapper around a bytestring.
///
/// This newtype only exists to provide a nicer `Debug` impl
#[derive(Clone, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct HtmlString(pub Vec<u8>);

impl Deref for HtmlString {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for HtmlString {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Debug for HtmlString {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "b\"")?;
        for &byte in &self.0 {
            for ch in std::ascii::escape_default(byte) {
                write!(f, "{}", ch as char)?;
            }
        }

        write!(f, "\"")
    }
}

impl Borrow<[u8]> for HtmlString {
    fn borrow(&self) -> &[u8] {
        &self.0
    }
}

impl BorrowMut<[u8]> for HtmlString {
    fn borrow_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }
}

impl AsRef<[u8]> for HtmlString {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl<const N: usize> PartialEq<&[u8; N]> for HtmlString {
    fn eq(&self, other: &&[u8; N]) -> bool {
        self.0 == *other
    }
}

impl<const N: usize> PartialEq<HtmlString> for &[u8; N] {
    fn eq(&self, other: &HtmlString) -> bool {
        other.0 == *self
    }
}

impl PartialEq<&[u8]> for HtmlString {
    fn eq(&self, other: &&[u8]) -> bool {
        self.0 == *other
    }
}

impl PartialEq<HtmlString> for &[u8] {
    fn eq(&self, other: &HtmlString) -> bool {
        *self == other.0
    }
}

impl PartialEq<Vec<u8>> for HtmlString {
    fn eq(&self, other: &Vec<u8>) -> bool {
        self.0 == *other
    }
}

impl PartialEq<HtmlString> for Vec<u8> {
    fn eq(&self, other: &HtmlString) -> bool {
        *self == other.0
    }
}

#[test]
fn test_eq_html_str_and_byte_literal() {
    assert!(HtmlString(b"hello world".to_vec()) == b"hello world");
}

#[test]
fn test_eq_byte_literal_and_html_str() {
    assert!(b"hello world" == HtmlString(b"hello world".to_vec()));
}

#[test]
fn test_eq_html_str_and_byte_slice() {
    assert!(HtmlString(b"hello world".to_vec()) == b"hello world".as_slice());
}

#[test]
fn test_eq_byte_slice_and_html_str() {
    assert!(b"hello world".as_slice() == HtmlString(b"hello world".to_vec()));
}

#[test]
fn test_borrowing() {
    use crate::StartTag;
    // demonstrate a usecase for Borrow/BorrowMut
    let tag = StartTag::default();
    assert!(!tag.attributes.contains_key(b"href".as_slice()));
}

impl From<Vec<u8>> for HtmlString {
    fn from(vec: Vec<u8>) -> HtmlString {
        HtmlString(vec)
    }
}

impl From<HtmlString> for Vec<u8> {
    fn from(other: HtmlString) -> Vec<u8> {
        other.0
    }
}
