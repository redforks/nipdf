use std::{collections::HashMap, iter::Peekable};

type Dictionary<'a> = HashMap<Name<'a>, Object<'a>>;
type Array<'a> = Vec<Object<'a>>;
type Stream<'a> = (Dictionary<'a>, &'a [u8]); // data part not including the stream/endstream keyword

#[derive(Clone, Copy, PartialEq, Debug, thiserror::Error)]
pub enum ObjectTypeError {
    #[error("unexpected type")]
    UnexpectedType,
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
    pub fn as_string(&self) -> Result<String, ObjectTypeError> {
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
            let mut iter = s.into_iter().copied().peekable();
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
            _ => Err(ObjectTypeError::UnexpectedType),
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

#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub struct Name<'a>(&'a [u8]);

#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub struct Reference {
    pub id: u32,
    pub generation: u16,
}

#[cfg(test)]
mod tests;
