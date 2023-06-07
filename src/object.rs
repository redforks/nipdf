//! Helpers for [`lopdf::Object`].
use lopdf::Object;
use pdf::{
    object::{GenNr, ObjNr, PlainRef},
    primitive::{Dictionary, Name, PdfString, Primitive},
};

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

pub fn new_pdf_string<I: Into<Vec<u8>>>(s: I) -> PdfString {
    PdfString::new(s.into().into())
}

/// Create Dictionary has one element  
pub fn new_dictionary1(n: impl Into<Name>, v: impl Into<Primitive>) -> Dictionary {
    let mut r = Dictionary::new();
    r.insert(n, v.into());
    r
}

/// Create Dictionary has two elements
pub fn new_dictionary2(
    n1: impl Into<Name>,
    v1: impl Into<Primitive>,
    n2: impl Into<Name>,
    v2: impl Into<Primitive>,
) -> Dictionary {
    let mut r = Dictionary::new();
    r.insert(n1, v1.into());
    r.insert(n2, v2.into());
    r
}

pub fn new_plain_ref(id: ObjNr, gen: GenNr) -> PlainRef {
    PlainRef { id, gen }
}
