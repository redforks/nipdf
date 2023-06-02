//! Helpers for [`lopdf::Object`].
use lopdf::Object;

pub trait NameAble {
    fn to_vec(self) -> Vec<u8>;
}

impl<'a> NameAble for &'a String {
    fn to_vec(self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }
}

impl<'a> NameAble for &'a str {
    fn to_vec(self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }
}

impl<'a> NameAble for &'a [u8] {
    fn to_vec(self) -> Vec<u8> {
        self.as_ref().to_vec()
    }
}

/// Create [`lopdf::Object::Name`] from [str] like `s`.
pub fn new_name(s: impl NameAble) -> Object {
    Object::Name(s.to_vec())
}

#[cfg(test)]
mod tests;
