//! Contains types of PDF file structures.

use std::borrow::Cow;

use crate::object::{Dictionary, Name, ObjectValueError};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Header<'a>(&'a [u8]);

impl<'a> Header<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self(buf)
    }

    pub fn ver(&self) -> &str {
        std::str::from_utf8(self.0).unwrap()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Catalog<'a> {
    dict: Dictionary<'a>,
}

impl<'a> Catalog<'a> {
    pub fn new(dict: Dictionary<'a>) -> Self {
        Self { dict }
    }

    pub fn ver(&self) -> Result<Option<Cow<[u8]>>, ObjectValueError> {
        self.dict
            .get(&Name::new(b"/Version".as_slice()))
            .map(|o| o.as_name())
            .transpose()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Trailer<'a> {
    dict: Dictionary<'a>,
}

impl<'a> Trailer<'a> {
    pub fn new(dict: Dictionary<'a>) -> Self {
        Self { dict }
    }

    pub fn total_objects(&self) -> Result<i32, ObjectValueError> {
        self.dict
            .get(&Name::new(b"/Size".as_slice()))
            .ok_or(ObjectValueError::DictNameMissing)
            .and_then(|obj| obj.as_int())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct File<'a> {
    content: &'a [u8],
    head: Header<'a>,
}

impl<'a> File<'a> {
    pub fn new(content: &'a [u8], head: Header<'a>) -> Self {
        Self { content, head }
    }
}

#[cfg(test)]
mod tests;
