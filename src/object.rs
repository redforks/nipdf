//! Helpers for [`lopdf::Object`].
use pdf::{
    object::{GenNr, ObjNr, PlainRef},
    primitive::{Dictionary, Name, PdfString, Primitive},
};

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
