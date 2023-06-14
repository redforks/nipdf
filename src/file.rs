//! Contains types of PDF file structures.

use crate::object::Dictionary;

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

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Tail {
    xref_pos: u32,
}

impl Tail {
    pub fn new(xref_pos: u32) -> Self {
        Self { xref_pos }
    }

    pub fn xref_pos(&self) -> u32 {
        self.xref_pos
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
}
