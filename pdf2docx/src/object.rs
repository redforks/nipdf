//! object mod contains data structure map to low level pdf objects
use ahash::HashMap;

use std::{
    borrow::{Borrow, Cow},
    iter::Peekable,
    str::from_utf8,
};

mod indirect_object;
pub use indirect_object::IndirectObject;
mod stream;
use once_cell::unsync::OnceCell;
pub use stream::*;

pub type Array<'a> = Vec<Object<'a>>;

#[derive(PartialEq, Debug, Clone, Default)]
pub struct Dictionary<'a>(HashMap<Name<'a>, Object<'a>>);

impl<'a> std::ops::DerefMut for Dictionary<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<'a> FromIterator<(Name<'a>, Object<'a>)> for Dictionary<'a> {
    fn from_iter<T: IntoIterator<Item = (Name<'a>, Object<'a>)>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl<'a> std::ops::Deref for Dictionary<'a> {
    type Target = HashMap<Name<'a>, Object<'a>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a> Dictionary<'a> {
    pub fn new() -> Self {
        Self(HashMap::default())
    }

    pub fn get_opt_int(&self, id: &str) -> Result<Option<i32>, ObjectValueError> {
        self.0
            .get(&id.into())
            .map_or(Ok(None), |o| o.as_int().map(Some))
    }

    pub fn get_int(&self, id: &str, default: i32) -> Result<i32, ObjectValueError> {
        self.0.get(&id.into()).map_or(Ok(default), |o| o.as_int())
    }

    pub fn get_bool(&self, id: &str, default: bool) -> Result<bool, ObjectValueError> {
        self.0.get(&id.into()).map_or(Ok(default), |o| o.as_bool())
    }

    pub fn set(&mut self, id: impl Into<Name<'a>>, value: impl Into<Object<'a>>) {
        self.0.insert(id.into(), value.into());
    }

    pub fn get_name(&self, id: &'static str) -> Result<Option<&str>, ObjectValueError> {
        self.0
            .get(&id.into())
            .map_or(Ok(None), |o| o.as_name().map(Some))
    }

    pub fn get_name_or(
        &self,
        id: &'static str,
        default: &'static str,
    ) -> Result<&str, ObjectValueError> {
        self.0.get(&id.into()).map_or(Ok(default), |o| o.as_name())
    }
}

pub trait SchemaTypeValidator {
    fn schema_type(&self) -> &'static str;
    fn check(&self, d: &Dictionary) -> Result<bool, ObjectValueError>;

    fn valid(&self, d: &Dictionary) -> Result<(), ObjectValueError> {
        self.check_result(self.check(d)?)
    }

    fn check_result(&self, result: bool) -> Result<(), ObjectValueError> {
        if result {
            Ok(())
        } else {
            Err(ObjectValueError::DictSchemaUnExpectedType(
                self.schema_type(),
            ))
        }
    }

    fn get_type<'a>(&self, d: &'a Dictionary) -> Result<&'a str, ObjectValueError> {
        let name = d
            .get_name("Type")
            .map_err(|_| ObjectValueError::DictSchemaError(self.schema_type(), "Type"))?;
        name.ok_or_else(|| ObjectValueError::DictSchemaError(self.schema_type(), "Type"))
    }

    fn get_type_opt<'a>(&self, d: &'a Dictionary) -> Result<Option<&'a str>, ObjectValueError> {
        d.get(&"Type".into()).map_or(Ok(None), |o| {
            o.as_name()
                .map(Some)
                .map_err(|_| ObjectValueError::DictSchemaError(self.schema_type(), "Type"))
        })
    }
}

impl SchemaTypeValidator for () {
    fn schema_type(&self) -> &'static str {
        "No Specific Type"
    }

    fn check(&self, _: &Dictionary) -> Result<bool, ObjectValueError> {
        Ok(true)
    }
}

impl SchemaTypeValidator for &'static str {
    fn schema_type(&self) -> &'static str {
        self
    }

    fn check(&self, d: &Dictionary) -> Result<bool, ObjectValueError> {
        Ok(*self == self.get_type(d)?)
    }
}

impl SchemaTypeValidator for Option<&'static str> {
    fn schema_type(&self) -> &'static str {
        self.expect("Should not happen")
    }

    fn check(&self, d: &Dictionary) -> Result<bool, ObjectValueError> {
        self.get_type_opt(d)?
            .map_or(Ok(true), |t| Ok(t == self.schema_type()))
    }
}

/// Check Type/Subtype fields. If first element is None, check Subtype only.
impl SchemaTypeValidator for (Option<&'static str>, &'static str) {
    fn schema_type(&self) -> &'static str {
        self.1
    }

    fn check(&self, d: &Dictionary) -> Result<bool, ObjectValueError> {
        fn get_sub_type<'a>(
            this: &(Option<&'static str>, &'static str),
            d: &'a Dictionary,
        ) -> Result<Option<&'a str>, ObjectValueError> {
            d.get_name("Subtype")
                .map_err(|_| ObjectValueError::DictSchemaError(this.schema_type(), "Subtype"))
        }

        if let Some(t) = self.0 {
            Ok(self.get_type_opt(d)?.map_or(true, |v| v == t)
                && Some(self.1) == get_sub_type(self, d)?)
        } else {
            Ok(Some(self.1) == get_sub_type(self, d)?)
        }
    }
}

impl<const N: usize> SchemaTypeValidator for [&'static str; N] {
    fn schema_type(&self) -> &'static str {
        self[0]
    }

    fn check(&self, d: &Dictionary) -> Result<bool, ObjectValueError> {
        Ok(self.contains(&self.get_type(d)?))
    }
}

pub struct SchemaDict<'a, 'b, T> {
    t: T,
    d: &'b Dictionary<'a>,
}

impl<'a, 'b, T: SchemaTypeValidator> SchemaDict<'a, 'b, T> {
    pub fn new(d: &'b Dictionary<'a>, t: T) -> Result<Self, ObjectValueError> {
        t.valid(d)?;
        Ok(Self { t, d })
    }

    pub fn from(d: &'b Dictionary<'a>, t: T) -> Result<Option<Self>, ObjectValueError> {
        if t.check(d)? {
            Ok(Some(Self { t, d }))
        } else {
            Ok(None)
        }
    }

    pub fn type_name(&self) -> &str {
        self.t.get_type(self.d).unwrap()
    }

    pub fn opt_name(&self, id: &'static str) -> Result<Option<&str>, ObjectValueError> {
        self.d
            .get(&id.into())
            .map_or(Ok(None), |o| o.as_name().map(Some))
    }

    pub fn required_name(&self, id: &'static str) -> Result<&str, ObjectValueError> {
        self.d
            .get(&id.into())
            .ok_or(ObjectValueError::DictSchemaError(self.t.schema_type(), id))?
            .as_name()
    }

    pub fn required_int(&self, id: &'static str) -> Result<i32, ObjectValueError> {
        self.d
            .get(&id.into())
            .ok_or(ObjectValueError::DictSchemaError(self.t.schema_type(), id))?
            .as_int()
    }

    pub fn opt_int(&self, id: &'static str) -> Result<Option<i32>, ObjectValueError> {
        self.d
            .get(&id.into())
            .map_or(Ok(None), |o| o.as_int().map(Some))
    }

    pub fn opt_bool(&self, id: &'static str) -> Result<Option<bool>, ObjectValueError> {
        self.d
            .get(&id.into())
            .map_or(Ok(None), |o| o.as_bool().map(Some))
    }

    pub fn required_bool(&self, id: &'static str) -> Result<bool, ObjectValueError> {
        self.d
            .get(&id.into())
            .ok_or(ObjectValueError::DictSchemaError(self.t.schema_type(), id))?
            .as_bool()
    }

    pub fn bool_or(&self, id: &'static str, default: bool) -> Result<bool, ObjectValueError> {
        self.opt_bool(id).map(|b| b.unwrap_or(default))
    }

    pub fn opt_u32(&self, id: &'static str) -> Result<Option<u32>, ObjectValueError> {
        self.opt_int(id).map(|i| i.map(|i| i as u32))
    }

    pub fn required_u32(&self, id: &'static str) -> Result<u32, ObjectValueError> {
        self.required_int(id).map(|i| i as u32)
    }

    pub fn u32_or(&self, id: &'static str, default: u32) -> Result<u32, ObjectValueError> {
        self.opt_u32(id).map(|i| i.unwrap_or(default))
    }

    pub fn opt_u8(&self, id: &'static str) -> Result<Option<u8>, ObjectValueError> {
        self.opt_int(id).map(|i| i.map(|i| i as u8))
    }

    pub fn required_u8(&self, id: &'static str) -> Result<u8, ObjectValueError> {
        self.required_int(id).map(|i| i as u8)
    }

    pub fn u8_or(&self, id: &'static str, default: u8) -> Result<u8, ObjectValueError> {
        self.opt_u8(id).map(|i| i.unwrap_or(default))
    }

    pub fn opt_rect(&self, id: &'static str) -> Result<Option<Rectangle>, ObjectValueError> {
        self.opt_arr(id).map(|arr| arr.map(|v| v.into()))
    }

    pub fn opt_f32(&self, id: &'static str) -> Result<Option<f32>, ObjectValueError> {
        self.d
            .get(&id.into())
            .map_or(Ok(None), |o| o.as_number().map(Some))
    }

    pub fn required_f32(&self, id: &'static str) -> Result<f32, ObjectValueError> {
        self.d
            .get(&id.into())
            .ok_or(ObjectValueError::DictSchemaError(self.t.schema_type(), id))?
            .as_number()
    }

    pub fn f32_or(&self, id: &'static str, default: f32) -> Result<f32, ObjectValueError> {
        self.opt_f32(id).map(|i| i.unwrap_or(default))
    }

    pub fn opt_object(&self, id: &'static str) -> Result<Option<&'b Object<'a>>, ObjectValueError> {
        Ok(self.d.get(&id.into()))
    }

    pub fn required_object(&self, id: &'static str) -> Result<&'b Object<'a>, ObjectValueError> {
        self.d
            .get(&id.into())
            .ok_or(ObjectValueError::DictSchemaError(self.t.schema_type(), id))
    }

    /// Return empty vec if not exist, error if not array
    pub fn u32_arr(&self, id: &'static str) -> Result<Vec<u32>, ObjectValueError> {
        self.opt_arr_map(id, |o| o.as_int().map(|i| i as u32))
            .map(|o| o.unwrap_or_default())
    }

    pub fn required_rect(&self, id: &'static str) -> Result<Rectangle, ObjectValueError> {
        self.opt_rect(id)?
            .ok_or(ObjectValueError::DictSchemaError(self.t.schema_type(), id))
    }

    pub fn rect_or(
        &self,
        id: &'static str,
        default: Rectangle,
    ) -> Result<Rectangle, ObjectValueError> {
        self.opt_rect(id).map(|r| r.unwrap_or(default))
    }

    pub fn required_arr_map<V>(
        &self,
        id: &'static str,
        f: impl Fn(&Object) -> Result<V, ObjectValueError>,
    ) -> Result<Vec<V>, ObjectValueError> {
        self.d
            .get(&id.into())
            .ok_or(ObjectValueError::DictSchemaError(self.t.schema_type(), id))?
            .as_arr()?
            .iter()
            .map(f)
            .collect()
    }

    pub fn opt_arr_map<V>(
        &self,
        id: &'static str,
        f: impl Fn(&Object) -> Result<V, ObjectValueError>,
    ) -> Result<Option<Vec<V>>, ObjectValueError> {
        self.d
            .get(&id.into())
            .map_or(Ok(None), |o| o.as_arr().map(Some))?
            .map(|arr| arr.iter().map(f).collect())
            .transpose()
    }

    pub fn opt_arr(&self, id: &'static str) -> Result<Option<&'b Array<'a>>, ObjectValueError> {
        self.d
            .get(&id.into())
            .map_or(Ok(None), |o| o.as_arr().map(Some))
    }

    /// Item can be a single object or an array of objects.
    /// If item not exist, returns empty vec.
    pub fn opt_single_or_arr<Item: 'b>(
        &self,
        id: &'static str,
        f: impl Fn(&'b Object<'a>) -> Result<Item, ObjectValueError>,
    ) -> Result<Vec<Item>, ObjectValueError> {
        self.d.get(&id.into()).map_or(Ok(Vec::new()), |o| match o {
            Object::Array(arr) => arr.iter().map(f).collect(),
            _ => f(o).map(|o| vec![o]),
        })
    }

    pub fn opt_dict(
        &self,
        id: &'static str,
    ) -> Result<Option<&'b Dictionary<'a>>, ObjectValueError> {
        self.d
            .get(&id.into())
            .map_or(Ok(None), |o| o.as_dict().map(Some))
    }

    pub fn required_dict(&self, id: &'static str) -> Result<&'b Dictionary<'a>, ObjectValueError> {
        self.opt_dict(id)
            .and_then(|o| o.ok_or(ObjectValueError::DictSchemaError(self.t.schema_type(), id)))
    }

    pub fn opt_rectangle(&self, id: &'static str) -> Result<Option<Rectangle>, ObjectValueError> {
        Ok(self.opt_arr(id)?.map(|arr| arr.into()))
    }

    pub fn required_ref(&self, id: &'static str) -> Result<u32, ObjectValueError> {
        self.d
            .get(&id.into())
            .ok_or(ObjectValueError::DictSchemaError(self.t.schema_type(), id))?
            .as_ref()
            .map(|r| r.id().id())
    }

    pub fn opt_ref(&self, id: &'static str) -> Result<Option<u32>, ObjectValueError> {
        self.d
            .get(&id.into())
            .map_or(Ok(None), |o| o.as_ref().map(|r| Some(r.id().id())))
    }

    pub fn ref_id_arr(&self, id: &'static str) -> Result<Vec<u32>, ObjectValueError> {
        self.opt_arr_map(id, |o| o.as_ref().map(|r| r.id().id()))
            .map(|o| o.unwrap_or_default())
    }
}

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

use crate::{file::Rectangle, parser};

#[derive(Clone, PartialEq, Debug, thiserror::Error)]
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
    #[error("Stream not image")]
    StreamNotImage,
    #[error("Stream is not bytes")]
    StreamIsNotBytes,
    #[error("Object not found by id")]
    ObjectIDNotFound,
    #[error("Parse error: {0}")]
    ParseError(String),
    #[error("Unexpected dict schema type, schema: {0}")]
    DictSchemaUnExpectedType(&'static str),
    #[error("Dict schema error, schema: {0}, key: {1}")]
    DictSchemaError(&'static str, &'static str),
    #[error("Graphics operation schema error")]
    GraphicsOperationSchemaError,
}

impl<'a> From<parser::ParseError<'a>> for ObjectValueError {
    fn from(e: parser::ParseError) -> Self {
        Self::ParseError(e.to_string())
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum Object<'a> {
    Null,
    Bool(bool),
    Integer(i32),
    Number(f32),
    LiteralString(LiteralString<'a>), // including the parentheses
    HexString(HexString<'a>),
    Name(Name<'a>), // with the leading slash
    Dictionary(Dictionary<'a>),
    Array(Array<'a>),
    Stream(Stream<'a>),
    Reference(Reference),
    // If `Length` is a reference, instead of int, can not parse stream without object resolver,
    // used inside object resolver as a intermediate state.
    LaterResolveStream(Dictionary<'a>),
}

impl Object<'static> {
    pub fn new_ref(id: u32) -> Self {
        Self::Reference(Reference::new(id, 0))
    }
}

impl<'a> Object<'a> {
    pub fn as_int(&self) -> Result<i32, ObjectValueError> {
        match self {
            Object::Integer(i) => Ok(*i),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }

    pub fn as_bool(&self) -> Result<bool, ObjectValueError> {
        match self {
            Object::Bool(b) => Ok(*b),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }

    pub fn as_number(&self) -> Result<f32, ObjectValueError> {
        match self {
            Object::Number(f) => Ok(*f),
            Object::Integer(v) => Ok(*v as f32),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }

    /// If value is a Name, return its normalized name, return error if
    /// value is not Name..
    pub fn as_name(&self) -> Result<&str, ObjectValueError> {
        match self {
            Object::Name(n) => Ok(from_utf8(n.0.borrow()).unwrap()),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }

    pub fn as_dict(&self) -> Result<&Dictionary<'a>, ObjectValueError> {
        match self {
            Object::Dictionary(d) => Ok(d),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }

    pub fn as_stream(&self) -> Result<&Stream<'a>, ObjectValueError> {
        match self {
            Object::Stream(s) => Ok(s),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }

    pub fn as_arr(&self) -> Result<&Array<'a>, ObjectValueError> {
        match self {
            Object::Array(a) => Ok(a),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }

    pub fn into_arr(self) -> Result<Array<'a>, ObjectValueError> {
        match self {
            Object::Array(a) => Ok(a),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }

    pub fn as_ref(&self) -> Result<&Reference, ObjectValueError> {
        match self {
            Object::Reference(r) => Ok(r),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }

    pub fn as_text_string(&self) -> Result<String, ObjectValueError> {
        match self {
            Object::LiteralString(s) => Ok(s.decoded()?.to_owned()),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }

    pub fn as_text_string_or_number(&self) -> Result<TextStringOrNumber, ObjectValueError> {
        match self {
            Object::LiteralString(s) => Ok(TextStringOrNumber::Text(s.decoded()?.to_owned())),
            Object::HexString(s) => Ok(TextStringOrNumber::HexText(s.0.to_owned())),
            Object::Number(n) => Ok(TextStringOrNumber::Number(*n)),
            Object::Integer(v) => Ok(TextStringOrNumber::Number(*v as f32)),
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

/// Convert [u8] to Object based on first char,
/// if start with '(' or '<', convert to LiteralString or HexString
/// if start with '/' convert to Name, panic otherwise
#[cfg(test)]
impl<'a> From<&'a [u8]> for Object<'a> {
    fn from(value: &'a [u8]) -> Self {
        assert!(!value.is_empty());
        match value[0] {
            b'(' => Self::LiteralString(LiteralString::new(value)),
            b'<' => Self::HexString(HexString::new(value)),
            b'/' => Self::Name((&value[1..]).into()),
            _ => panic!("invalid object"),
        }
    }
}

/// Convert &str to Object based on first char,
/// if start with '(' or '<', convert to LiteralString or HexString
/// if start with '/' convert to Name, panic otherwise
#[cfg(test)]
impl<'a> From<&'a str> for Object<'a> {
    fn from(value: &'a str) -> Self {
        value.as_bytes().into()
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

#[derive(Eq, PartialEq, Debug, Clone)]
pub struct LiteralString<'a>(&'a [u8], OnceCell<Cow<'a, str>>);

impl<'a> From<&'a [u8]> for LiteralString<'a> {
    fn from(s: &'a [u8]) -> Self {
        Self(s, OnceCell::new())
    }
}

impl<'a> From<&'a str> for LiteralString<'a> {
    fn from(value: &'a str) -> Self {
        Self(value.as_bytes(), OnceCell::new())
    }
}

impl<'a> LiteralString<'a> {
    pub fn new(s: &'a [u8]) -> Self {
        Self(s, OnceCell::new())
    }

    pub fn decoded(&self) -> Result<&str, ObjectValueError> {
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

        Ok(self
            .1
            .get_or_init(|| {
                let s = self.0;
                let s = &s[1..s.len() - 1];
                let mut result = String::with_capacity(s.len());
                let mut iter = s.iter().copied().peekable();
                // TODO: use exist buf if no escape, or newline to normalize
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
                            // TODO: test escape new line
                            if skip_cur_new_line(next, &mut iter) {
                                result.push('\n');
                            } else {
                                result.push(next as char);
                            }
                        }
                    }
                }

                result.into()
            })
            .borrow())
    }
}

impl<'a> From<LiteralString<'a>> for Object<'a> {
    fn from(value: LiteralString<'a>) -> Self {
        Self::LiteralString(value)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TextStringOrNumber {
    Text(String),
    // maybe CID font
    HexText(Vec<u8>),
    Number(f32),
}

/// Decoded PDF literal string object, enclosing '(' and ')' not included.
#[derive(Eq, PartialEq, Debug, Clone)]
pub struct HexString<'a>(&'a [u8], OnceCell<Vec<u8>>);

impl<'a> From<&'a [u8]> for HexString<'a> {
    fn from(s: &'a [u8]) -> Self {
        Self::new(s)
    }
}

impl<'a> From<&'a str> for HexString<'a> {
    fn from(value: &'a str) -> Self {
        Self::new(value.as_bytes())
    }
}

impl<'a> HexString<'a> {
    pub fn new(s: &'a [u8]) -> Self {
        Self(s, OnceCell::new())
    }

    /// Get decoded binary string.
    pub fn decoded(&self) -> Result<&[u8], ObjectValueError> {
        self.1
            .get_or_try_init(|| {
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
                let s = self.0;
                debug_assert!(s.starts_with(b"<") && s.ends_with(b">"));
                let s = &s[1..s.len() - 1];
                let s = filter_whitespace(s);
                let s = append_zero_if_odd(&s);

                hex::decode(s).map_err(|_| ObjectValueError::InvalidHexString)
            })
            .map(|s| &s[..])
    }
}

impl<'a> From<HexString<'a>> for Object<'a> {
    fn from(value: HexString<'a>) -> Self {
        Self::HexString(value)
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
