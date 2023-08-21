//! object mod contains data structure map to low level pdf objects
use ahash::HashMap;
use educe::Educe;

use std::{
    borrow::{Borrow, Cow},
    fmt::{Debug, Display},
    iter::Peekable,
    num::NonZeroU32,
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

    pub fn get_opt_int_ref(&self, id: &str) -> Result<Option<&i32>, ObjectValueError> {
        self.0
            .get(id.as_bytes())
            .map_or(Ok(None), |o| o.as_int_ref().map(Some))
    }

    pub fn get_int(&self, id: &str, default: i32) -> Result<i32, ObjectValueError> {
        self.0
            .get(id.as_bytes())
            .map_or(Ok(default), |o| o.as_int())
    }

    pub fn get_bool(&self, id: &str, default: bool) -> Result<bool, ObjectValueError> {
        self.0
            .get(id.as_bytes())
            .map_or(Ok(default), |o| o.as_bool())
    }

    pub fn set(&mut self, id: impl Into<Name<'a>>, value: impl Into<Object<'a>>) {
        self.0.insert(id.into(), value.into());
    }

    pub fn get_name(&self, id: &'static str) -> Result<Option<&str>, ObjectValueError> {
        self.0
            .get(id.as_bytes())
            .map_or(Ok(None), |o| o.as_name().map(Some))
    }

    pub fn get_name_or(
        &self,
        id: &'static str,
        default: &'static str,
    ) -> Result<&str, ObjectValueError> {
        self.0
            .get(id.as_bytes())
            .map_or(Ok(default), |o| o.as_name())
    }
}

/// Get type value from Dictionary.
pub trait TypeValueGetter {
    type Value: ?Sized;
    /// Return None if type value is not exist
    fn get<'a>(&self, d: &'a Dictionary) -> Result<Option<&'a Self::Value>, ObjectValueError>;
    /// Type field name
    fn field(&self) -> &'static str;
}

/// Implement `TypeValueGetter` returns i32 value
#[derive(Debug, Clone)]
pub struct IntTypeValueGetter {
    field: &'static str,
}

impl IntTypeValueGetter {
    pub fn new(field: &'static str) -> Self {
        Self { field }
    }
}

impl TypeValueGetter for IntTypeValueGetter {
    type Value = i32;

    fn get<'a>(&self, d: &'a Dictionary) -> Result<Option<&'a Self::Value>, ObjectValueError> {
        d.get_opt_int_ref(self.field)
    }

    fn field(&self) -> &'static str {
        self.field
    }
}

/// Implement `TypeValueGetter` returns str value
#[derive(Debug, Clone)]
pub struct NameTypeValueGetter {
    field: &'static str,
}

impl NameTypeValueGetter {
    pub fn new(field: &'static str) -> Self {
        Self { field }
    }
}

impl TypeValueGetter for NameTypeValueGetter {
    type Value = str;

    fn get<'a>(&self, d: &'a Dictionary) -> Result<Option<&'a Self::Value>, ObjectValueError> {
        d.get_name(self.field)
    }

    fn field(&self) -> &'static str {
        self.field
    }
}

pub trait TypeValueCheck<V: ?Sized>: Clone + Debug {
    fn schema_type(&self) -> Cow<str>;
    fn check(&self, v: Option<&V>) -> bool;

    /// Convert current checker to `OptionTypeValueChecker`, return `true` if value is `None`.
    fn option(self) -> OptionTypeValueChecker<Self>
    where
        Self: Sized,
    {
        OptionTypeValueChecker(self)
    }
}

#[derive(Clone, Debug)]
pub struct EqualTypeValueChecker<R: Debug + Clone> {
    value: R,
}

impl<R: Debug + Clone> EqualTypeValueChecker<R> {
    pub fn new(s: R) -> Self {
        Self { value: s }
    }
}

impl<R: Borrow<str> + Debug + Clone> TypeValueCheck<str> for EqualTypeValueChecker<R> {
    fn schema_type(&self) -> Cow<str> {
        Cow::Borrowed(self.value.borrow())
    }

    fn check(&self, v: Option<&str>) -> bool {
        v.map_or(false, |v| v == self.value.borrow())
    }
}

impl TypeValueCheck<i32> for EqualTypeValueChecker<i32> {
    fn schema_type(&self) -> Cow<str> {
        Cow::Owned(self.value.to_string())
    }

    fn check(&self, v: Option<&i32>) -> bool {
        v.map_or(false, |v| *v == self.value)
    }
}

/// impl `TypeValueCheck` return true if value is None, otherwise check value using `inner`.
#[derive(Clone, Debug)]
pub struct OptionTypeValueChecker<Inner: Sized + Clone + Debug>(pub Inner);

impl<Inner: TypeValueCheck<V> + Clone + Debug, V: ?Sized> TypeValueCheck<V>
    for OptionTypeValueChecker<Inner>
{
    fn schema_type(&self) -> Cow<str> {
        self.0.schema_type()
    }

    fn check(&self, v: Option<&V>) -> bool {
        v.map_or(true, |v| self.0.check(Some(v)))
    }
}

/// Check type value equals to one of `values`.
#[derive(Clone, Debug)]
pub struct OneOfTypeValueChecker<R: Clone + Debug> {
    values: Vec<R>,
}

impl<R: Clone + Debug> OneOfTypeValueChecker<R> {
    pub fn new(values: Vec<R>) -> Self {
        debug_assert!(!values.is_empty());
        Self { values }
    }
}

impl<V: Display + ?Sized + PartialEq, R: Borrow<V> + Clone + Debug> TypeValueCheck<V>
    for OneOfTypeValueChecker<R>
{
    fn schema_type(&self) -> Cow<str> {
        Cow::Owned(
            self.values
                .iter()
                .map(|v| v.borrow().to_string())
                .collect::<Vec<_>>()
                .join("|"),
        )
    }

    fn check(&self, v: Option<&V>) -> bool {
        v.map_or(false, |v| self.values.iter().any(|r| v == r.borrow()))
    }
}

/// Check type value to validate object Type.
pub trait TypeValidator: Debug + Clone {
    fn schema_type(&self) -> String;
    fn check(&self, d: &Dictionary) -> Result<bool, ObjectValueError>;

    fn valid(&self, d: &Dictionary) -> Result<(), ObjectValueError> {
        if self.check(d)? {
            Ok(())
        } else {
            Err(ObjectValueError::DictSchemaUnExpectedType(
                self.schema_type(),
            ))
        }
    }
}

impl TypeValidator for () {
    fn schema_type(&self) -> String {
        "Empty type validator".to_owned()
    }

    fn check(&self, _: &Dictionary) -> Result<bool, ObjectValueError> {
        Ok(true)
    }
}

#[derive(Debug, Clone)]
/// Implement `TypeValidator` using `TypeValueGetter` and `TypeValueChecker`
pub struct ValueTypeValidator<G: Debug + Clone, C: Debug + Clone> {
    getter: G,
    checker: C,
}

impl<G: Debug + Clone, C: Debug + Clone> ValueTypeValidator<G, C> {
    pub fn new(getter: G, checker: C) -> Self {
        Self { getter, checker }
    }
}

impl<G, C, V: ?Sized> TypeValidator for ValueTypeValidator<G, C>
where
    G: TypeValueGetter<Value = V> + Debug + Clone,
    C: TypeValueCheck<V> + Debug + Clone,
{
    fn schema_type(&self) -> String {
        format!("{}: {}", self.getter.field(), self.checker.schema_type())
    }

    fn check(&self, d: &Dictionary) -> Result<bool, ObjectValueError> {
        let v = self.getter.get(d)?;
        Ok(self.checker.check(v))
    }
}

/// If both validator is valid, then the value is valid.
#[derive(Debug, Clone)]
pub struct AndValueTypeValidator<V1, V2>
where
    V1: Clone + Debug,
    V2: Clone + Debug,
{
    v1: V1,
    v2: V2,
}

impl<V1, V2> AndValueTypeValidator<V1, V2>
where
    V1: Clone + Debug,
    V2: Clone + Debug,
{
    pub fn new(v1: V1, v2: V2) -> Self {
        Self { v1, v2 }
    }
}

impl<V1, V2> TypeValidator for AndValueTypeValidator<V1, V2>
where
    V1: Clone + Debug + TypeValidator,
    V2: Clone + Debug + TypeValidator,
{
    fn schema_type(&self) -> String {
        format!("{} and {}", self.v1.schema_type(), self.v2.schema_type())
    }

    fn check(&self, d: &Dictionary) -> Result<bool, ObjectValueError> {
        self.v1.check(d).and(self.v2.check(d))
    }
}

#[derive(Clone, Educe)]
#[educe(Debug)]
pub struct SchemaDict<'a, 'b, T: Clone + Debug> {
    t: T,
    d: &'b Dictionary<'a>,
    #[educe(Debug(ignore))]
    r: &'b ObjectResolver<'a>,
}

pub trait PdfObject<'a, 'b>
where
    Self: Sized,
{
    fn new(
        id: Option<NonZeroU32>,
        dict: &'b Dictionary<'a>,
        r: &'b ObjectResolver<'a>,
    ) -> Result<Self, ObjectValueError>;

    fn checked(
        id: Option<NonZeroU32>,
        dict: &'b Dictionary<'a>,
        r: &'b ObjectResolver<'a>,
    ) -> Result<Option<Self>, ObjectValueError>;

    fn id(&self) -> Option<NonZeroU32>;
}

impl<'a, 'b, T: TypeValidator> SchemaDict<'a, 'b, T> {
    pub fn new(
        d: &'b Dictionary<'a>,
        r: &'b ObjectResolver<'a>,
        t: T,
    ) -> Result<Self, ObjectValueError> {
        t.valid(d)?;
        Ok(Self { t, d, r })
    }

    pub fn from(
        d: &'b Dictionary<'a>,
        r: &'b ObjectResolver<'a>,
        t: T,
    ) -> Result<Option<Self>, ObjectValueError> {
        if t.check(d)? {
            Ok(Some(Self { t, d, r }))
        } else {
            Ok(None)
        }
    }

    pub fn dict(&self) -> &'b Dictionary<'a> {
        self.d
    }

    pub fn resolver(&self) -> &'b ObjectResolver<'a> {
        self.r
    }

    fn opt_get(&self, id: &'static str) -> Result<Option<&'b Object<'a>>, ObjectValueError> {
        self.r.opt_resolve_container_value(self.d, id)
    }

    pub fn opt_name(&self, id: &'static str) -> Result<Option<&'b str>, ObjectValueError> {
        self.opt_get(id)?
            .map_or(Ok(None), |o| o.as_name().map(Some))
    }

    pub fn required_name(&self, id: &'static str) -> Result<&'b str, ObjectValueError> {
        self.opt_get(id)?
            .ok_or(ObjectValueError::DictSchemaError(self.t.schema_type(), id))?
            .as_name()
    }

    pub fn required_int(&self, id: &'static str) -> Result<i32, ObjectValueError> {
        self.opt_get(id)?
            .ok_or(ObjectValueError::DictSchemaError(self.t.schema_type(), id))?
            .as_int()
    }

    pub fn opt_int(&self, id: &'static str) -> Result<Option<i32>, ObjectValueError> {
        self.opt_get(id)?.map_or(Ok(None), |o| o.as_int().map(Some))
    }

    pub fn opt_bool(&self, id: &'static str) -> Result<Option<bool>, ObjectValueError> {
        self.opt_get(id)?
            .map_or(Ok(None), |o| o.as_bool().map(Some))
    }

    pub fn required_bool(&self, id: &'static str) -> Result<bool, ObjectValueError> {
        self.opt_get(id)?
            .ok_or(ObjectValueError::DictSchemaError(self.t.schema_type(), id))?
            .as_bool()
    }

    pub fn bool_or(&self, id: &'static str, default: bool) -> Result<bool, ObjectValueError> {
        self.opt_bool(id).map(|b| b.unwrap_or(default))
    }

    pub fn opt_u16(&self, id: &'static str) -> Result<Option<u16>, ObjectValueError> {
        self.opt_int(id).map(|i| i.map(|i| i as u16))
    }

    pub fn required_u16(&self, id: &'static str) -> Result<u16, ObjectValueError> {
        self.required_int(id).map(|i| i as u16)
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

    pub fn opt_f32(&self, id: &'static str) -> Result<Option<f32>, ObjectValueError> {
        self.opt_get(id)?
            .map_or(Ok(None), |o| o.as_number().map(Some))
    }

    pub fn required_f32(&self, id: &'static str) -> Result<f32, ObjectValueError> {
        self.opt_get(id)?
            .ok_or(ObjectValueError::DictSchemaError(self.t.schema_type(), id))?
            .as_number()
    }

    pub fn f32_or(&self, id: &'static str, default: f32) -> Result<f32, ObjectValueError> {
        self.opt_f32(id).map(|i| i.unwrap_or(default))
    }

    pub fn opt_object(&self, id: &'static str) -> Result<Option<&'b Object<'a>>, ObjectValueError> {
        self.opt_get(id)
    }

    pub fn required_object(&self, id: &'static str) -> Result<&'b Object<'a>, ObjectValueError> {
        self.opt_object(id)?
            .ok_or(ObjectValueError::DictSchemaError(self.t.schema_type(), id))
    }

    /// Return empty vec if not exist, error if not array
    pub fn u32_arr(&self, id: &'static str) -> Result<Vec<u32>, ObjectValueError> {
        self.opt_arr_map(id, |o| o.as_int().map(|i| i as u32))
            .map(|o| o.unwrap_or_default())
    }

    /// Return empty vec if not exist, error if not array
    pub fn f32_arr(&self, id: &'static str) -> Result<Vec<f32>, ObjectValueError> {
        self.opt_arr_map(id, |o| o.as_number())
            .map(|o| o.unwrap_or_default())
    }

    pub fn required_arr_map<V>(
        &self,
        id: &'static str,
        f: impl Fn(&Object) -> Result<V, ObjectValueError>,
    ) -> Result<Vec<V>, ObjectValueError> {
        self.opt_get(id)?
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
        self.opt_get(id)?
            .map_or(Ok(None), |o| o.as_arr().map(Some))?
            .map(|arr| arr.iter().map(f).collect())
            .transpose()
    }

    pub fn opt_arr(&self, id: &'static str) -> Result<Option<&'b Array<'a>>, ObjectValueError> {
        self.opt_get(id)?.map_or(Ok(None), |o| o.as_arr().map(Some))
    }

    /// Item can be a single object or an array of objects.
    /// If item not exist, returns empty vec.
    pub fn opt_single_or_arr<Item>(
        &self,
        id: &'static str,
        f: impl Fn(&Object<'a>) -> Result<Item, ObjectValueError>,
    ) -> Result<Vec<Item>, ObjectValueError> {
        self.d
            .get(id.as_bytes())
            .map_or(Ok(Vec::new()), |o| match o {
                Object::Array(arr) => arr.iter().map(f).collect(),
                _ => f(o).map(|o| vec![o]),
            })
    }

    pub fn opt_single_or_arr_stream(
        &self,
        id: &'static str,
    ) -> Result<Vec<&Stream<'a>>, ObjectValueError> {
        let resolver = self.resolver();
        match resolver.resolve_container_value(self.d, id)? {
            Object::Array(arr) => arr
                .iter()
                .map(|o| resolver.resolve_reference(o)?.as_stream())
                .collect(),
            o => resolver.resolve_reference(o)?.as_stream().map(|o| vec![o]),
        }
    }

    pub fn opt_dict(
        &self,
        id: &'static str,
    ) -> Result<Option<&'b Dictionary<'a>>, ObjectValueError> {
        self.opt_get(id)?
            .map_or(Ok(None), |o| o.as_dict().map(Some))
    }

    pub fn required_dict(&self, id: &'static str) -> Result<&'b Dictionary<'a>, ObjectValueError> {
        self.opt_dict(id)
            .and_then(|o| o.ok_or(ObjectValueError::DictSchemaError(self.t.schema_type(), id)))
    }

    pub fn required_ref(&self, id: &'static str) -> Result<NonZeroU32, ObjectValueError> {
        self.d
            .get(id.as_bytes())
            .ok_or(ObjectValueError::DictSchemaError(self.t.schema_type(), id))?
            .as_ref()
            .map(|r| r.id().id())
    }

    pub fn opt_ref(&self, id: &'static str) -> Result<Option<NonZeroU32>, ObjectValueError> {
        self.d
            .get(id.as_bytes())
            .map_or(Ok(None), |o| o.as_ref().map(|r| Some(r.id().id())))
    }

    pub fn ref_id_arr(&self, id: &'static str) -> Result<Vec<NonZeroU32>, ObjectValueError> {
        self.opt_arr_map(id, |o| o.as_ref().map(|r| r.id().id()))
            .map(|o| o.unwrap_or_default())
    }
}

#[derive(PartialEq, Eq, Hash, Debug, Clone, Copy)]
pub struct ObjectId {
    id: NonZeroU32,
    generation: u16,
}

impl ObjectId {
    pub fn new(id: NonZeroU32, generation: u16) -> Self {
        Self { id, generation }
    }

    pub fn id(&self) -> NonZeroU32 {
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

use crate::{file::ObjectResolver, parser};

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
    DictSchemaUnExpectedType(String),
    #[error("Dict schema error, schema: {0}, key: {1}")]
    DictSchemaError(String, &'static str),
    #[error("Graphics operation schema error")]
    GraphicsOperationSchemaError,
    #[error("Dict key not found")]
    DictKeyNotFound,
}

impl<'a> From<parser::ParseError<'a>> for ObjectValueError {
    fn from(e: parser::ParseError) -> Self {
        Self::ParseError(format!("{:?}", e))
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
}

impl Object<'static> {
    pub fn new_ref(id: u32) -> Self {
        Self::Reference(Reference::new_u32(id, 0))
    }
}

impl<'a> Object<'a> {
    pub fn as_int(&self) -> Result<i32, ObjectValueError> {
        match self {
            Object::Integer(i) => Ok(*i),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }

    pub fn as_int_ref(&self) -> Result<&i32, ObjectValueError> {
        match self {
            Object::Integer(i) => Ok(i),
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

#[cfg(feature = "pretty")]
use pretty::RcDoc;

#[cfg(feature = "pretty")]
impl<'a> Object<'a> {
    pub fn to_doc(&self) -> RcDoc {
        fn name_to_doc<'a>(n: &'a Name) -> RcDoc<'a> {
            RcDoc::text("/").append(RcDoc::text(n.as_ref()))
        }

        fn dict_to_doc<'a>(d: &'a Dictionary) -> RcDoc<'a> {
            let mut keys = d.keys().collect::<Vec<_>>();
            keys.sort();
            RcDoc::text("<<")
                .append(
                    RcDoc::intersperse(
                        keys.into_iter()
                            .map(|k| {
                                name_to_doc(k)
                                    .append(RcDoc::space())
                                    .append(d.get(k).unwrap().to_doc())
                            })
                            .collect::<Vec<_>>(),
                        RcDoc::line(),
                    )
                    .nest(2)
                    .group(),
                )
                .append(RcDoc::text(">>"))
        }

        match self {
            Object::Null => RcDoc::text("null"),
            Object::Bool(b) => RcDoc::text(if *b { "true" } else { "false" }),
            Object::Integer(i) => RcDoc::as_string(i),
            Object::Number(f) => RcDoc::as_string(PrettyNumber(*f)),
            Object::LiteralString(s) => RcDoc::text(from_utf8(s.0).unwrap()),
            Object::HexString(s) => RcDoc::text(from_utf8(s.0).unwrap()),
            Object::Name(n) => name_to_doc(n),
            Object::Dictionary(d) => dict_to_doc(d),
            Object::Array(a) => RcDoc::text("[")
                .append(RcDoc::intersperse(
                    a.iter().map(|o| o.to_doc()),
                    RcDoc::space(),
                ))
                .append(RcDoc::text("]")),
            Object::Stream(s) => dict_to_doc(s.as_dict())
                .append(RcDoc::line())
                .append(RcDoc::text("<<stream>>")),
            Object::Reference(r) => RcDoc::as_string(r.id().id())
                .append(RcDoc::space())
                .append(RcDoc::as_string(r.id().generation()))
                .append(RcDoc::space())
                .append(RcDoc::text("R")),
        }
    }
}

#[cfg(feature = "pretty")]
struct PrettyNumber(f32);

#[cfg(feature = "pretty")]
impl Display for PrettyNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
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
#[derive(Eq, PartialEq, Hash, Debug, Clone, PartialOrd, Ord)]
pub struct Name<'a>(pub Cow<'a, [u8]>);

/// Name can borrow to &[u8], for use as key in HashMap.
///
/// Note: do not impl `Borrow<str>` for Name, because it will use
/// different hash algorithm, which may not get value from HashMap.
impl<'a> Borrow<[u8]> for Name<'a> {
    fn borrow(&self) -> &[u8] {
        &self.0
    }
}

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

impl<'a> AsRef<str> for Name<'a> {
    fn as_ref(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap()
    }
}

#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub struct Reference(ObjectId);

impl Reference {
    pub fn new(id: NonZeroU32, generation: u16) -> Self {
        Self(ObjectId::new(id, generation))
    }

    /// Panic if id is Zero
    pub fn new_u32(id: u32, generation: u16) -> Self {
        Self(ObjectId::new(NonZeroU32::new(id).unwrap(), generation))
    }

    pub fn id(&self) -> ObjectId {
        self.0
    }
}

#[cfg(test)]
impl<'a> From<u32> for Object<'a> {
    fn from(value: u32) -> Self {
        Self::Reference(Reference::new_u32(value, 0))
    }
}

#[cfg(test)]
mod tests;
