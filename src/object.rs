//! object mod contains data structure map to low level pdf objects
use ahash::HashMap;

use std::{
    borrow::{Borrow, Cow},
    iter::Peekable,
};

mod indirect_object;
pub use indirect_object::IndirectObject;
mod stream;
pub use stream::*;

pub type Dictionary<'a> = HashMap<Name<'a>, Object<'a>>;
pub type Array<'a> = Vec<Object<'a>>;

#[derive(PartialEq, Eq, Hash, Debug, Clone, Copy)]
pub struct ObjectId {
    id: u32,
    generation: u16,
}

impl ObjectId {
    pub fn new(id: u32, generation: u16) -> Self {
        Self { id, generation }
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn generation(&self) -> u16 {
        self.generation
    }
}

mod xref;
pub use xref::{Entry as XRefEntry, Section as XRefSection, *};

mod frame;
pub use frame::*;

#[derive(Clone, Copy, PartialEq, Debug, thiserror::Error)]
pub enum ObjectValueError {
    #[error("unexpected type")]
    UnexpectedType,
    #[error("invalid hex string")]
    InvalidHexString,
    #[error("invalid name format")]
    InvalidNameFormat,
    #[error("Name not in dictionary")]
    DictNameMissing,
    #[error("Reference target not found")]
    ReferenceTargetNotFound,
    #[error("External stream not supported")]
    ExternalStreamNotSupported,
    #[error("Unknown filter")]
    UnknownFilter,
    #[error("Filter decode error")]
    FilterDecodeError,
}

#[derive(Clone, PartialEq, Debug)]
pub enum Object<'a> {
    Null,
    Bool(bool),
    Integer(i32),
    Number(f32),
    LiteralString(&'a [u8]), // including the parentheses
    HexString(&'a [u8]),     // including the angle brackets
    Name(Name<'a>),          // with the leading slash
    Dictionary(Dictionary<'a>),
    Array(Array<'a>),
    Stream(Stream<'a>),
    Reference(Reference),
}

impl<'a> Object<'a> {
    /// decode LiteralString and HexString to String
    pub fn as_string(&self) -> Result<String, ObjectValueError> {
        fn skip_cur_new_line<I: Iterator<Item = u8>>(cur: u8, s: &mut Peekable<I>) -> bool {
            if cur == b'\r' {
                s.next_if_eq(&b'\n');
                true
            } else if cur == b'\n' {
                s.next_if_eq(&b'\r');
                true
            } else {
                false
            }
        }

        fn skip_next_line<I: Iterator<Item = u8>>(s: &mut Peekable<I>) -> bool {
            if s.next_if_eq(&b'\r').is_some() {
                s.next_if_eq(&b'\n');
                true
            } else if s.next_if_eq(&b'\n').is_some() {
                s.next_if_eq(&b'\r');
                true
            } else {
                false
            }
        }

        fn next_oct_char<I: Iterator<Item = u8>>(s: &mut Peekable<I>) -> Option<u8> {
            let mut result = 0;
            let mut hit = false;
            for _ in 0..3 {
                if let Some(c) = s.next_if(|v| matches!(v, b'0'..=b'7')) {
                    hit = true;
                    result = result * 8 + (c - b'0');
                }
            }
            hit.then_some(result)
        }

        fn decode_literal_string(s: &[u8]) -> String {
            let s = &s[1..s.len() - 1];
            let mut result = String::with_capacity(s.len());
            let mut iter = s.iter().copied().peekable();
            while let Some(next) = iter.next() {
                match next {
                    b'\\' => {
                        if skip_next_line(&mut iter) {
                            continue;
                        }
                        if let Some(ch) = next_oct_char(&mut iter) {
                            result.push(ch as char);
                            continue;
                        }

                        if let Some(c) = iter.next() {
                            match c {
                                b'r' => result.push('\r'),
                                b'n' => result.push('\n'),
                                b't' => result.push('\t'),
                                b'f' => result.push('\x0c'),
                                b'b' => result.push('\x08'),
                                b'(' => result.push('('),
                                b')' => result.push(')'),
                                _ => result.push(c as char),
                            }
                        }
                    }
                    _ => {
                        if skip_cur_new_line(next, &mut iter) {
                            result.push('\n');
                        } else {
                            result.push(next as char);
                        }
                    }
                }
            }

            result
        }

        match self {
            Object::LiteralString(s) => Ok(decode_literal_string(s)),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }

    fn as_hex_string(&self) -> Result<Vec<u8>, ObjectValueError> {
        fn decode_hex_string(s: &[u8]) -> Result<Vec<u8>, ObjectValueError> {
            let s = &s[1..s.len() - 1];

            fn filter_whitespace(s: &[u8]) -> Cow<[u8]> {
                if s.iter().copied().any(|b| b.is_ascii_whitespace()) {
                    Cow::Owned(
                        s.iter()
                            .copied()
                            .filter(|b| !b.is_ascii_whitespace())
                            .collect::<Vec<_>>(),
                    )
                } else {
                    Cow::Borrowed(s)
                }
            }
            fn append_zero_if_odd(s: &[u8]) -> Cow<[u8]> {
                if s.len() % 2 == 0 {
                    Cow::Borrowed(s)
                } else {
                    let mut v = Vec::with_capacity(s.len() + 1);
                    v.extend_from_slice(s);
                    v.push(b'0');
                    Cow::Owned(v)
                }
            }
            let s = filter_whitespace(s);
            let s = append_zero_if_odd(&s);

            hex::decode(s).map_err(|_| ObjectValueError::InvalidHexString)
        }

        match self {
            Object::HexString(s) => decode_hex_string(s),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }

    pub fn as_int(&self) -> Result<i32, ObjectValueError> {
        match self {
            Object::Integer(i) => Ok(*i),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }

    pub fn as_number(&self) -> Result<f32, ObjectValueError> {
        match self {
            Object::Number(f) => Ok(*f),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }

    /// If value is a Name, return its normalized name, return error if
    /// value is not Name..
    pub fn as_name(&self) -> Result<&[u8], ObjectValueError> {
        match self {
            Object::Name(n) => Ok(n.0.borrow()),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }

    pub fn as_dict(&self) -> Result<&Dictionary<'a>, ObjectValueError> {
        match self {
            Object::Dictionary(d) => Ok(d),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }

    pub fn as_reference(&self) -> Result<&Reference, ObjectValueError> {
        match self {
            Object::Reference(r) => Ok(r),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }
}

impl<'a> From<Stream<'a>> for Object<'a> {
    fn from(value: Stream<'a>) -> Self {
        Self::Stream(value)
    }
}

impl<'a> From<Array<'a>> for Object<'a> {
    fn from(value: Array<'a>) -> Self {
        Self::Array(value)
    }
}

impl<'a> From<Reference> for Object<'a> {
    fn from(value: Reference) -> Self {
        Self::Reference(value)
    }
}

impl<'a> From<Dictionary<'a>> for Object<'a> {
    fn from(value: Dictionary<'a>) -> Self {
        Self::Dictionary(value)
    }
}

impl<'a> From<Name<'a>> for Object<'a> {
    fn from(value: Name<'a>) -> Self {
        Self::Name(value)
    }
}

impl<'a> From<&'a [u8]> for Object<'a> {
    fn from(value: &'a [u8]) -> Self {
        Self::LiteralString(value)
    }
}

impl<'a> From<f32> for Object<'a> {
    fn from(value: f32) -> Self {
        Self::Number(value)
    }
}

impl<'a> From<i32> for Object<'a> {
    fn from(value: i32) -> Self {
        Self::Integer(value)
    }
}

impl<'a> From<bool> for Object<'a> {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

/// A PDF name object, preceding '/' not included.
#[derive(Eq, PartialEq, Hash, Debug, Clone)]
pub struct Name<'a>(pub Cow<'a, [u8]>);

impl<'a> From<&'a str> for Name<'a> {
    fn from(value: &'a str) -> Self {
        Self(Cow::Borrowed(value.as_bytes()))
    }
}

impl<'a> From<&'a [u8]> for Name<'a> {
    fn from(value: &'a [u8]) -> Self {
        Self(Cow::Borrowed(value))
    }
}

impl<'a> Name<'a> {
    pub fn borrowed(v: &'a [u8]) -> Self {
        debug_assert!(!v.starts_with(b"/"));
        Self(Cow::Borrowed(v))
    }

    pub fn owned(v: Vec<u8>) -> Self {
        debug_assert!(!v.starts_with(b"/"));
        Self(Cow::Owned(v))
    }
}

#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub struct Reference(ObjectId);

impl Reference {
    pub fn new(id: u32, generation: u16) -> Self {
        Self(ObjectId::new(id, generation))
    }

    pub fn id(&self) -> ObjectId {
        self.0
    }
}

#[cfg(test)]
mod tests;
