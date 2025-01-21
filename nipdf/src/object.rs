//! object mod contains data structure map to low level pdf objects
use crate::{ObjectValueError, Result, file::ObjectResolver};
use ahash::{HashMap, HashMapExt};
use educe::Educe;
use itertools::Itertools as _;
use paste::paste;
use prescript::Name;
use std::{
    borrow::{Borrow, Cow},
    fmt::{Debug, Display},
    iter::Peekable,
    rc::Rc,
    str::{Utf8Error, from_utf8},
};
use tinyvec::TinyVec;

mod stream;
pub use stream::*;
pub type Array = Rc<[Object]>;
use snafu::{OptionExt as _, ResultExt as _, whatever};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct IndirectObjectDef(pub(crate) ObjectId, pub(crate) Object);

impl IndirectObjectDef {
    pub fn id(&self) -> ObjectId {
        self.0
    }

    pub fn take(self) -> Object {
        self.1
    }

    pub fn to_runtime_object_id(&self) -> RuntimeObjectId {
        self.into()
    }
}

impl From<IndirectObjectDef> for RuntimeObjectId {
    fn from(value: IndirectObjectDef) -> Self {
        value.0.id
    }
}

impl From<&IndirectObjectDef> for RuntimeObjectId {
    fn from(value: &IndirectObjectDef) -> Self {
        value.0.id
    }
}

#[derive(PartialEq, Debug, Clone, Default, Educe)]
#[educe(Deref, DerefMut)]
pub struct Dictionary(Rc<HashMap<Name, Object>>);

impl FromIterator<(Name, Object)> for Dictionary {
    fn from_iter<T: IntoIterator<Item = (Name, Object)>>(iter: T) -> Self {
        Self(Rc::new(iter.into_iter().collect()))
    }
}

impl Dictionary {
    pub fn new() -> Self {
        Self(Rc::new(HashMap::default()))
    }

    pub fn from(d: HashMap<Name, Object>) -> Self {
        Self(Rc::new(d))
    }

    pub fn with_capacity(n: usize) -> Self {
        Self(Rc::new(HashMap::with_capacity(n)))
    }

    pub fn update<T>(&mut self, f: impl FnOnce(&mut HashMap<Name, Object>) -> T) -> T {
        f(Rc::make_mut(&mut self.0))
    }
}

/// Get type value from Dictionary.
pub trait TypeValueGetter {
    type Value;
    /// Return None if type value is not exist
    fn get(&self, d: &Dictionary) -> Result<Option<Self::Value>, ObjectValueError>;
    /// Type field name
    fn field(&self) -> &Name;
}

/// Implement `TypeValueGetter` returns i32 value
#[derive(Debug, Clone)]
pub struct IntTypeValueGetter {
    field: Name,
}

impl IntTypeValueGetter {
    pub fn new(field: Name) -> Self {
        Self { field }
    }
}

impl TypeValueGetter for IntTypeValueGetter {
    type Value = i32;

    fn get(&self, d: &Dictionary) -> Result<Option<i32>, ObjectValueError> {
        d.get(&self.field).map_or(Ok(None), |o| o.int().map(Some))
    }

    fn field(&self) -> &Name {
        &self.field
    }
}

/// Implement `TypeValueGetter` returns str value
#[derive(Debug, Clone)]
pub struct NameTypeValueGetter {
    field: Name,
}

impl NameTypeValueGetter {
    pub fn new(field: Name) -> Self {
        Self { field }
    }
}

impl TypeValueGetter for NameTypeValueGetter {
    type Value = Name;

    fn get(&self, d: &Dictionary) -> Result<Option<Name>, ObjectValueError> {
        d.get(&self.field).map(Object::name).transpose()
    }

    fn field(&self) -> &Name {
        &self.field
    }
}

pub trait TypeValueCheck<V>: Clone + Debug {
    fn schema_type(&self) -> Cow<'_, str>;
    fn check(&self, v: Option<V>) -> bool;

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

impl TypeValueCheck<Name> for EqualTypeValueChecker<Name> {
    fn schema_type(&self) -> Cow<'_, str> {
        Cow::Borrowed(self.value.as_str())
    }

    fn check(&self, v: Option<Name>) -> bool {
        v.as_ref() == Some(&self.value)
    }
}

impl TypeValueCheck<i32> for EqualTypeValueChecker<i32> {
    fn schema_type(&self) -> Cow<'_, str> {
        Cow::Owned(self.value.to_string())
    }

    fn check(&self, v: Option<i32>) -> bool {
        v.as_ref() == Some(&self.value)
    }
}

/// impl `TypeValueCheck` return true if value is None, otherwise check value using `inner`.
#[derive(Clone, Debug)]
pub struct OptionTypeValueChecker<Inner: Sized + Clone + Debug>(pub Inner);

impl<Inner: TypeValueCheck<V> + Clone + Debug, V> TypeValueCheck<V>
    for OptionTypeValueChecker<Inner>
{
    fn schema_type(&self) -> Cow<'_, str> {
        self.0.schema_type()
    }

    fn check(&self, v: Option<V>) -> bool {
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

impl<V: Display + PartialEq + Clone + Debug> TypeValueCheck<V> for OneOfTypeValueChecker<V> {
    fn schema_type(&self) -> Cow<'_, str> {
        Cow::Owned(
            self.values
                .iter()
                .map(|v| v.borrow().to_string())
                .collect::<Vec<_>>()
                .join("|"),
        )
    }

    fn check(&self, v: Option<V>) -> bool {
        v.is_some_and(|v| self.values.iter().any(|r| &v == r))
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
            Err(ObjectValueError::DictSchemaUnExpectedType {
                schema: self.schema_type(),
            })
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

impl<G, C, V> TypeValidator for ValueTypeValidator<G, C>
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

mod private {
    pub struct Root;
    pub struct Embedded;
}
pub(crate) use private::*;

pub trait PdfObjectCore<'a, 'b> {
    fn dict(&self) -> &'b Dictionary;

    fn resolver(&self) -> &'b ObjectResolver<'a>;
}

/// PdfObject that has reference id
pub trait RootPdfObject<'a, 'b>: PdfObjectCore<'a, 'b>
where
    Self: Sized,
{
    fn new(
        id: RuntimeObjectId,
        dict: &'b Dictionary,
        r: &'b ObjectResolver<'a>,
    ) -> Result<Self, ObjectValueError>;

    fn id(&self) -> RuntimeObjectId;
}

/// PdfObject that has embbed in other container object, such as Dictionary or Array.
pub trait PdfObject<'a, 'b>: PdfObjectCore<'a, 'b>
where
    Self: Sized,
{
    fn new(dict: &'b Dictionary, r: &'b ObjectResolver<'a>) -> Result<Self, ObjectValueError>;
}

pub trait ToPdfObject<T> {
    fn to_pdf_object(&self) -> Result<T, ObjectValueError>;
}

impl<'a: 'b, 'b, F, T> ToPdfObject<(T, Root, Root)> for F
where
    F: RootPdfObject<'a, 'b>,
    T: RootPdfObject<'a, 'b>,
{
    fn to_pdf_object(&self) -> Result<(T, Root, Root), ObjectValueError> {
        T::new(self.id(), self.dict(), self.resolver()).map(|o| (o, Root, Root))
    }
}

impl<'a: 'b, 'b, F, T> ToPdfObject<(T, Root, Embedded)> for F
where
    F: RootPdfObject<'a, 'b>,
    T: PdfObject<'a, 'b>,
{
    fn to_pdf_object(&self) -> Result<(T, Root, Embedded), ObjectValueError> {
        T::new(self.dict(), self.resolver()).map(|o| (o, Root, Embedded))
    }
}

impl<'a: 'b, 'b, F, T> ToPdfObject<(T, Embedded, Embedded)> for F
where
    F: PdfObject<'a, 'b>,
    T: PdfObject<'a, 'b>,
{
    fn to_pdf_object(&self) -> Result<(T, Embedded, Embedded), ObjectValueError> {
        T::new(self.dict(), self.resolver()).map(|o| (o, Embedded, Embedded))
    }
}

#[derive(Educe)]
#[educe(Debug, Clone)]
pub struct SchemaDict<'a, 'b, T: Clone + Debug> {
    t: T,
    d: &'b Dictionary,
    #[educe(Debug(ignore))]
    r: &'b ObjectResolver<'a>,
}

impl<'a, 'b, T: TypeValidator> SchemaDict<'a, 'b, T> {
    pub fn new(
        d: &'b Dictionary,
        r: &'b ObjectResolver<'a>,
        t: T,
    ) -> Result<Self, ObjectValueError> {
        t.valid(d)?;
        Ok(Self { t, d, r })
    }

    pub fn dict(&self) -> &'b Dictionary {
        self.d
    }

    pub fn resolver(&self) -> &'b ObjectResolver<'a> {
        self.r
    }
}

/// Trait to abstract creation of PdfObject / RootPdfObject
pub trait FromSchemaContainer<'a, 'b>: Sized {
    fn create(o: &'b Object, r: &'b ObjectResolver<'a>) -> Result<Self, ObjectValueError>;
}

/// impl CreateFromSchemaDict for type impl TryFrom trait
macro_rules! create_from_schema_dict_try_from {
    ($t: ty) => {
        impl<'a, 'b> FromSchemaContainer<'a, 'b> for $t {
            fn create(o: &'b Object, r: &'b ObjectResolver<'a>) -> Result<Self, ObjectValueError> {
                let o = r.resolve_reference(o)?;
                <$t>::try_from(o)
            }
        }
    };
}
create_from_schema_dict_try_from!(u8);
create_from_schema_dict_try_from!(u16);
create_from_schema_dict_try_from!(RuntimeObjectId);
create_from_schema_dict_try_from!(i32);
create_from_schema_dict_try_from!(u32);
create_from_schema_dict_try_from!(f32);
create_from_schema_dict_try_from!(bool);
create_from_schema_dict_try_from!(Name);
create_from_schema_dict_try_from!(&'b Dictionary);
create_from_schema_dict_try_from!(&'b Stream);
create_from_schema_dict_try_from!(&'b str);
create_from_schema_dict_try_from!(&'b [u8]);

impl<'a, 'b, T> FromSchemaContainer<'a, 'b> for Vec<T>
where
    T: FromSchemaContainer<'b, 'b>,
{
    fn create(o: &'b Object, r: &'b ObjectResolver<'a>) -> Result<Self, ObjectValueError> {
        let o = r.resolve_reference(o)?;
        let arr = o.as_arr()?;
        arr.iter().map(|o| T::create(o, r)).collect()
    }
}

impl<'a, 'b, T: TypeValidator> SchemaDict<'a, 'b, T> {
    pub fn required<V>(&self, key: &Name) -> Result<V>
    where
        V: FromSchemaContainer<'a, 'b>,
    {
        let r = self.resolver();
        self.required_value(key).and_then(move |o| V::create(o, r))
    }

    pub fn opt<V>(&self, key: &Name) -> Result<Option<V>>
    where
        V: FromSchemaContainer<'a, 'b>,
    {
        self.d
            .get(key)
            .map(|o| V::create(o, self.resolver()))
            .transpose()
    }

    /// Return default value if not exist, error if not expected type.
    pub fn or_default<V>(&self, key: &Name) -> Result<V>
    where
        V: FromSchemaContainer<'a, 'b> + Default,
    {
        self.opt(key).map(Option::unwrap_or_default)
    }

    /// If value not exist, return empty vector.
    /// If value is array, return all elements in array, otherwise return with one element vec.
    pub fn zero_one_or_more<V>(&self, key: &Name) -> Result<Vec<V>>
    where
        V: FromSchemaContainer<'a, 'b>,
    {
        let with_err =
            |_: &mut ObjectValueError| format!("resolve zero_one_or_more porerty: {}", &key);
        let v = self.d.get(key);
        match v {
            Some(v @ Object::Reference(reference)) => {
                let o = self
                    .r
                    .resolve(reference)
                    .with_whatever_context::<_, _, ObjectValueError>(with_err)?;
                match o {
                    Object::Array(arr) => arr
                        .iter()
                        .map(|o| V::create(o, self.resolver()))
                        .collect::<Result<_, _>>()
                        .with_whatever_context::<_, _, ObjectValueError>(with_err),
                    _ => Ok(vec![
                        V::create(v, self.resolver())
                            .with_whatever_context::<_, _, ObjectValueError>(with_err)?,
                    ]),
                }
            }
            Some(Object::Array(arr)) => arr
                .iter()
                .map(|o| V::create(o, self.resolver()))
                .collect::<Result<_, _>>()
                .with_whatever_context::<_, _, ObjectValueError>(with_err),
            Some(o) => Ok(vec![
                V::create(o, self.resolver())
                    .with_whatever_context::<_, _, ObjectValueError>(with_err)?,
            ]),
            None => Ok(vec![]),
        }
    }

    pub fn map_dict<V>(&self, key: &Name) -> Result<HashMap<Name, V>>
    where
        V: FromSchemaContainer<'a, 'b>,
    {
        let v = self.opt_object(key)?;
        let Some(v) = v else {
            return Ok(HashMap::new());
        };
        let v = v
            .as_dict()
            .whatever_context::<_, ObjectValueError>("as dict")?;
        let mut res = HashMap::with_capacity(v.len());
        for (k, v) in v.iter() {
            res.insert(k.clone(), V::create(v, self.resolver())?);
        }
        Ok(res)
    }

    pub fn opt_object(&self, key: &Name) -> Result<Option<&'b Object>> {
        let Some(v) = self.d.get(key) else {
            return Ok(None);
        };
        self.r
            .resolve_reference(v)
            .map(Some)
            .with_whatever_context(|_| format!("resolve reference for key: {}", key))
    }

    pub fn required_object(&self, key: &Name) -> Result<&'b Object> {
        self.required_value(key)
            .and_then(|o| self.r.resolve_reference(o))
    }

    fn required_value(&self, key: &Name) -> Result<&'b Object> {
        self.d
            .get(key)
            .with_whatever_context(|| format!("required value for key: {}", key))
    }
}

/// Object and resolver, to allow resolve reference from object
pub struct ObjectWithResolver<'a, 'b> {
    pub obj: &'b Object,
    pub resolver: &'b ObjectResolver<'a>,
}

#[cfg(test)]
pub(crate) fn try_from<
    T: for<'a, 'b> TryFrom<ObjectWithResolver<'a, 'b>, Error = ObjectValueError>,
>(
    o: Object,
) -> Result<T> {
    let xref = crate::file::XRefTable::empty();
    let resolver = ObjectResolver::empty(&xref);
    let o = ObjectWithResolver::new(&o, &resolver)?;
    T::try_from(o)
}

impl<'a, 'b> ObjectWithResolver<'a, 'b> {
    /// Create ObjectWithResolver from object and resolver, return error if resolve reference
    /// failed.
    pub fn new(obj: &'b Object, resolver: &'b ObjectResolver<'a>) -> Result<Self> {
        let obj = resolver.resolve_reference(obj)?;
        Ok(Self { obj, resolver })
    }

    fn resolve_reference(&self) -> Result<&'b Object> {
        self.resolver.resolve_reference(self.obj)
    }

    /// Expect self is Dictionary, convert self to SchemaDict
    pub fn into_schema_dict(self) -> Result<SchemaDict<'a, 'b, ()>> {
        let d = self.resolve_reference()?.as_dict()?;
        SchemaDict::new(d, self.resolver, ())
    }

    pub fn into_schema_array(self) -> Result<SchemaArray<'a, 'b>> {
        let arr = self.resolve_reference()?.as_arr()?;
        Ok(SchemaArray::new(arr, self.resolver))
    }

    pub fn into_object(self) -> &'b Object {
        self.obj
    }
}

impl<'a, 'b> FromSchemaContainer<'a, 'b> for ObjectWithResolver<'a, 'b> {
    fn create(o: &'b Object, r: &'b ObjectResolver<'a>) -> Result<Self, ObjectValueError> {
        ObjectWithResolver::new(o, r)
    }
}

pub struct SchemaArray<'a, 'b> {
    arr: &'b Array,
    resolver: &'b ObjectResolver<'a>,
}

impl<'a, 'b> SchemaArray<'a, 'b> {
    pub fn new(arr: &'b Array, resolver: &'b ObjectResolver<'a>) -> Self {
        Self { arr, resolver }
    }

    /// Get value at index, error if index out of bounds or wrong type
    pub fn required<V>(&self, index: usize) -> Result<V>
    where
        V: FromSchemaContainer<'a, 'b>,
    {
        let o = self
            .arr
            .get(index)
            .with_whatever_context::<_, _, ObjectValueError>(|| {
                format!("index out of bounds: {}", index)
            })?;
        V::create(o, self.resolver)
    }

    /// Get optional value at index, None if index out of bounds
    pub fn opt<V>(&self, index: usize) -> Result<Option<V>>
    where
        V: FromSchemaContainer<'a, 'b>,
    {
        match self.arr.get(index) {
            Some(o) => V::create(o, self.resolver).map(Some),
            None => Ok(None),
        }
    }

    pub fn required_schema_dict(&self, index: usize) -> Result<SchemaDict<'a, 'b, ()>> {
        let o = self.required_object(index)?;
        let d = o.as_dict()?;
        SchemaDict::new(d, self.resolver, ())
    }

    pub fn opt_schema_dict(&self, index: usize) -> Result<Option<SchemaDict<'a, 'b, ()>>> {
        match self.opt_object(index)? {
            Some(o) => {
                let d = o.as_dict()?;
                SchemaDict::new(d, self.resolver, ()).map(Some)
            }
            None => Ok(None),
        }
    }

    /// Get object at index after resolving any references
    pub fn required_object(&self, index: usize) -> Result<&'b Object> {
        let o = self
            .arr
            .get(index)
            .with_whatever_context::<_, _, ObjectValueError>(|| {
                format!("index out of bounds: {}", index)
            })?;
        self.resolver.resolve_reference(o)
    }

    /// Get optional object at index after resolving references
    pub fn opt_object(&self, index: usize) -> Result<Option<&'b Object>> {
        match self.arr.get(index) {
            Some(o) => self.resolver.resolve_reference(o).map(Some),
            None => Ok(None),
        }
    }

    /// Get length of array
    pub fn len(&self) -> usize {
        self.arr.len()
    }

    /// Check if array is empty
    pub fn is_empty(&self) -> bool {
        self.arr.is_empty()
    }

    /// Get iterator over array items, each item is resolved for possible reference
    pub fn iter(&self) -> impl Iterator<Item = Result<&Object>> {
        self.arr.iter().map(|o| self.resolver.resolve_reference(o))
    }

    /// Try to convert all items to type V
    pub fn to_vec<V>(&self) -> Result<Vec<V>>
    where
        V: FromSchemaContainer<'a, 'b>,
    {
        self.arr
            .iter()
            .map(|o| V::create(o, self.resolver))
            .collect::<Result<Vec<_>, _>>()
    }
}

/// Object id has id and generation, at runtime, generation
/// is not used, RuntimeObjectId removes generation to save space.
#[derive(PartialEq, Eq, Hash, Debug, Clone, Copy)]
pub struct RuntimeObjectId(pub u32);

impl Borrow<u32> for RuntimeObjectId {
    fn borrow(&self) -> &u32 {
        &self.0
    }
}

impl Display for RuntimeObjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u32> for RuntimeObjectId {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

#[derive(PartialEq, Eq, Hash, Debug, Clone, Copy)]
pub struct ObjectId {
    id: RuntimeObjectId,
    generation: u16,
}

impl ObjectId {
    pub fn new(id: impl Into<RuntimeObjectId>, generation: u16) -> Self {
        Self {
            id: id.into(),
            generation,
        }
    }

    #[cfg(test)]
    pub fn empty() -> Self {
        Self {
            id: 1.into(),
            generation: 0,
        }
    }

    pub fn id(&self) -> RuntimeObjectId {
        self.id
    }

    pub fn generation(&self) -> u16 {
        self.generation
    }
}

mod xref;
pub use xref::{Entry as XRefEntry, *};

mod frame;
use crate::graphics::trans::ThousandthsOfText;
pub use frame::*;

/// Pdf basic object types.
///
/// Access methods:
///
///   1. For Copy types: `typename() -> Result<$type, ObjectValueError>`, return error if value not
///      expected type.
///   1. For Reference types: `as_typename() -> Result<&$type, ObjectValueError>`, return error if
///      value not expected type.
///   1. `opt_typename() -> Option<$type>`, return None if value not specific type, for both Copy
///      and Reference types.
#[derive(Clone, PartialEq, Debug)]
pub enum Object {
    Null,
    Bool(bool),
    Integer(i32),
    Number(f32),
    LiteralString(LiteralString),
    HexString(HexString),
    Name(Name),
    Dictionary(Dictionary),
    Array(Array),
    Stream(Stream),
    Reference(Reference),
}

/// Macro to implement TryFrom<&Object> for various types
macro_rules! impl_try_from_object {
    // For simple types that directly use Object methods
    ($type:ty, $method:ident) => {
        impl TryFrom<&Object> for $type {
            type Error = ObjectValueError;

            fn try_from(value: &Object) -> Result<Self, Self::Error> {
                value.$method()
            }
        }
    };

    // For types that need type casting
    ($type:ty, $method:ident, $cast:expr) => {
        impl TryFrom<&Object> for $type {
            type Error = ObjectValueError;

            fn try_from(value: &Object) -> Result<Self, Self::Error> {
                value.$method().map($cast)
            }
        }
    };

    // For reference types
    ($type:ty, $as_method:ident, ref) => {
        impl<'a> TryFrom<&'a Object> for &'a $type {
            type Error = ObjectValueError;

            fn try_from(value: &'a Object) -> Result<Self, Self::Error> {
                value.$as_method()
            }
        }
    };
}

impl_try_from_object!(i32, int);
impl_try_from_object!(bool, bool);
impl_try_from_object!(Reference, reference);
impl_try_from_object!(u16, int, |v: i32| v as u16);
impl_try_from_object!(u32, int, |v: i32| v as u32);
impl_try_from_object!(u8, int, |v: i32| v as u8);
impl_try_from_object!(RuntimeObjectId, reference, Into::into);
impl_try_from_object!(f32, number);
impl_try_from_object!(Name, name);
impl_try_from_object!(Dictionary, as_dict, ref);
impl_try_from_object!(Stream, as_stream, ref);
impl_try_from_object!(str, as_str, ref);
impl_try_from_object!([u8], as_bstr, ref);

impl<T> TryFrom<&Object> for Vec<T>
where
    T: Copy + for<'a> TryFrom<&'a Object, Error = ObjectValueError>,
{
    type Error = ObjectValueError;

    fn try_from(value: &Object) -> Result<Self, Self::Error> {
        value.as_arr()?.iter().map(T::try_from).collect()
    }
}

macro_rules! copy_value_access {
    ($method:ident, $branch:ident, $t:ty) => {
        impl Object {
            paste! {
                /// Return `ObjectValueError::UnexpectedType` if value not expected type.
                pub fn $method(&self) -> Result<$t, ObjectValueError> {
                    match self {
                        Self::$branch(v) => Ok(*v),
                        _ => Err(ObjectValueError::UnexpectedType),
                    }
                }
            }
        }
    };
}

macro_rules! ref_value_access {
    ($method:ident, $branch:ident, $t:ty) => {
        impl Object {
            paste! {
                /// Return `ObjectValueError::UnexpectedType` if value not expected type.
                pub fn [<as_ $method>](&self) -> Result<$t, ObjectValueError> {
                    match self {
                        Self::$branch(v) => Ok(&v),
                        _ => Err(ObjectValueError::UnexpectedType),
                    }
                }
            }
        }
    };
}

copy_value_access!(bool, Bool, bool);
copy_value_access!(reference, Reference, Reference);
ref_value_access!(arr, Array, &Array);
ref_value_access!(stream, Stream, &Stream);

impl Object {
    #[cfg(test)]
    pub fn new_ref(id: u32) -> Self {
        Self::Reference(Reference::new(id, 0))
    }

    /// Return `ObjectValueError::UnexpectedType` if value not expected type.
    pub fn name(&self) -> Result<Name, ObjectValueError> {
        match self {
            Self::Name(v) => Ok(v.clone()),
            _ => whatever!("Expected name or integer, got {:?}", self),
        }
    }

    pub fn as_name(&self) -> Result<&Name, ObjectValueError> {
        match self {
            Self::Name(v) => Ok(v),
            _ => whatever!("Expected name or integer, got {:?}", self),
        }
    }

    /// Get number as i32, if value is f32, convert to i32, error otherwise.
    pub fn int(&self) -> Result<i32, ObjectValueError> {
        match self {
            Object::Integer(v) => Ok(*v),
            Object::Number(v) => v
                .to_i32()
                .whatever_context::<_, ObjectValueError>("convert f32 to int"),
            _ => whatever!("Expected number or integer, got {:?}", self),
        }
    }

    pub fn number(&self) -> Result<f32, ObjectValueError> {
        match self {
            Object::Number(v) => Ok(*v),
            Object::Integer(v) => Ok(*v as f32),
            _ => whatever!("Expected number or integer, got {:?}", self),
        }
    }

    pub fn as_dict(&self) -> Result<&Dictionary, ObjectValueError> {
        match self {
            Object::Dictionary(d) => Ok(d),
            Object::Stream(s) => Ok(s.as_dict()),
            _ => whatever!("Expected stream or dictionary, got {:?}", self),
        }
    }

    /// Return decoded string from LiteralString or HexString
    pub fn as_str(&self) -> Result<&str, ObjectValueError> {
        match self {
            Object::LiteralString(s) => s.as_str(),
            Object::HexString(s) => s
                .as_str()
                .whatever_context::<_, ObjectValueError>("convert HexString to str"),
            _ => whatever!("Expected LiteralString or HexString, got {:?}", self),
        }
    }

    /// Decode Literal string and hex string into bytes.
    pub fn as_bstr(&self) -> Result<&[u8], ObjectValueError> {
        match self {
            Object::LiteralString(s) => Ok(s.as_bytes()),
            Object::HexString(s) => Ok(s.as_bytes()),
            _ => whatever!("Expected LiteralString or HexString, got {:?}", self),
        }
    }

    /// iter values of current object recursively, for array recursively iter item values,
    /// for Dictionary iter values (key are ignored), other object types return itself.
    pub fn iter_values(&self) -> Box<dyn Iterator<Item = &'_ Self> + '_> {
        match self {
            Object::Array(a) => Box::new(a.iter().flat_map(|o| o.iter_values())),
            Object::Dictionary(d) => Box::new(d.values().flat_map(|o| o.iter_values())),
            Object::Stream(s) => Box::new(s.as_dict().values().flat_map(|o| o.iter_values())),
            _ => Box::new(std::iter::once(self)),
        }
    }

    /// Update array items in place, if array is shared, clone it first.
    pub(crate) fn update_array_items(arr: &mut Array, mut f: impl FnMut(&mut Object)) {
        match Rc::get_mut(arr) {
            Some(r) => {
                for o in r.iter_mut() {
                    f(o);
                }
            }
            None => {
                let mut ar: Vec<_> = arr.iter().cloned().collect();
                for o in &mut ar {
                    f(o);
                }
                let ar: Rc<[Object]> = ar.into();
                *arr = ar;
            }
        }
    }

    pub(crate) fn try_update_array_items(
        arr: &mut Array,
        mut f: impl FnMut(&mut Object) -> Result<()>,
    ) -> Result<()> {
        match Rc::get_mut(arr) {
            Some(r) => {
                for o in r.iter_mut() {
                    f(o)?;
                }
            }
            None => {
                let mut ar: Vec<_> = arr.iter().cloned().collect();
                for o in &mut ar {
                    f(o)?;
                }
                let ar: Rc<[Object]> = ar.into();
                *arr = ar;
            }
        }
        Ok(())
    }
}

impl<const N: usize> TryFrom<ObjectWithResolver<'_, '_>> for [f32; N] {
    type Error = ObjectValueError;

    fn try_from(obj: ObjectWithResolver<'_, '_>) -> Result<Self, Self::Error> {
        let arr = obj.into_schema_array()?;
        if arr.len() != N {
            return Err(ObjectValueError::UnexpectedType);
        }
        let mut r = [0.0; N];
        for (i, item) in r.iter_mut().enumerate().take(N) {
            *item = arr.required_object(i)?.number()?;
        }
        Ok(r)
    }
}

use euclid::Length;
use num_traits::ToPrimitive;
use pretty::RcDoc;
use static_assertions::assert_eq_size;

impl Object {
    pub fn to_doc(&self) -> RcDoc<'_> {
        fn name_to_doc(n: &Name) -> RcDoc<'_> {
            RcDoc::text("/").append(RcDoc::text(n.as_str()))
        }

        fn dict_to_doc(d: &Dictionary) -> RcDoc<'_> {
            RcDoc::text("<<")
                .append(
                    RcDoc::intersperse(
                        d.iter()
                            .sorted_by_key(|(k, _)| *k)
                            .map(|(k, v)| name_to_doc(k).append(RcDoc::space()).append(v.to_doc())),
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
            Object::LiteralString(s) => RcDoc::text(from_utf8(&s.0).map_or_else(
                |_| format!("0x{}", hex::encode(s.as_bytes())),
                |s| format!("({})", s),
            )),
            Object::HexString(s) => RcDoc::text(format!("<{}>", hex::encode(s.as_bytes()))),
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
            Object::Reference(r) => RcDoc::as_string(r.to_runtime_object_id())
                .append(RcDoc::space())
                .append(RcDoc::as_string(r.id().generation()))
                .append(RcDoc::space())
                .append(RcDoc::text("R")),
        }
    }
}

struct PrettyNumber(f32);

impl Display for PrettyNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub struct LiteralString(pub(crate) InnerString);

impl LiteralString {
    // TODO: remove convert escape logic, winnow version parser handled it
    pub fn new(s: &[u8]) -> Self {
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

        fn next_oct_byte<I: Iterator<Item = u8>>(s: &mut Peekable<I>) -> Option<u8> {
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

        let s = &s[1..s.len() - 1];
        let mut result: InnerString = InnerString::with_capacity(s.len());
        let mut iter = s.iter().copied().peekable();

        // TODO: use exist buf if no escape, or newline to normalize
        while let Some(next) = iter.next() {
            match next {
                b'\\' => {
                    if skip_next_line(&mut iter) {
                        continue;
                    }
                    if let Some(b) = next_oct_byte(&mut iter) {
                        result.push(b);
                        continue;
                    }

                    if let Some(b) = iter.next() {
                        match b {
                            b'r' => result.push(b'\r'),
                            b'n' => result.push(b'\n'),
                            b't' => result.push(b'\t'),
                            b'f' => result.push(b'\x0c'),
                            b'b' => result.push(b'\x08'),
                            b'(' => result.push(b'('),
                            b')' => result.push(b')'),
                            _ => result.push(b),
                        }
                    }
                }
                _ => {
                    // TODO: test escape new line
                    if skip_cur_new_line(next, &mut iter) {
                        result.push(b'\n');
                    } else {
                        result.push(next);
                    }
                }
            }
        }

        Self(result)
    }

    pub fn update(&mut self, f: impl FnOnce(&mut [u8])) {
        f(self.0.as_mut_slice());
    }

    pub fn as_str(&self) -> Result<&str, ObjectValueError> {
        from_utf8(&self.0).whatever_context("LiteralString as str")
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TextString {
    Text(LiteralString),
    // maybe CID font
    HexText(HexString),
}

impl TextString {
    pub fn to_bytes(&self) -> &[u8] {
        match self {
            TextString::Text(s) => s.as_bytes(),
            TextString::HexText(s) => s.as_bytes(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TextStringOrNumber {
    TextString(TextString),
    Number(Length<f32, ThousandthsOfText>),
}

pub(crate) type InnerString = TinyVec<[u8; 14]>;

/// Decoded PDF literal string object, enclosing '(' and ')' not included.
#[derive(Eq, PartialEq, Debug, Clone)]
pub struct HexString(pub(crate) InnerString);
assert_eq_size!(HexString, TinyVec<[u8; 14]>, (u64, u64, u64));

impl HexString {
    pub fn update(&mut self, f: impl FnOnce(&mut [u8])) {
        f(self.0.as_mut_slice());
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_ref()
    }

    pub fn as_str(&self) -> Result<&str, Utf8Error> {
        from_utf8(self.as_bytes())
    }
}

#[derive(PartialEq, Eq, Hash, Debug, Clone, Copy)]
pub struct Reference(ObjectId);

impl Reference {
    pub fn new(id: impl Into<RuntimeObjectId>, generation: u16) -> Self {
        Self(ObjectId::new(id, generation))
    }

    pub fn id(&self) -> ObjectId {
        self.0
    }

    pub fn to_runtime_object_id(self) -> RuntimeObjectId {
        self.into()
    }
}

impl From<Reference> for RuntimeObjectId {
    fn from(reference: Reference) -> Self {
        reference.id().id
    }
}

impl From<&Reference> for RuntimeObjectId {
    fn from(reference: &Reference) -> Self {
        reference.id().id
    }
}

/// Macro to implement From<T> for Object for various types
macro_rules! impl_from_object {
    // For types that directly map to an Object variant
    ($type:ty, $variant:ident) => {
        impl From<$type> for Object {
            fn from(value: $type) -> Self {
                Self::$variant(value)
            }
        }
    };
}

#[cfg(test)]
impl_from_object!(f32, Number);
#[cfg(test)]
impl_from_object!(i32, Integer);
#[cfg(test)]
impl_from_object!(bool, Bool);
#[cfg(test)]
impl_from_object!(Array, Array);
#[cfg(test)]
impl_from_object!(Name, Name);
#[cfg(test)]
impl_from_object!(HexString, HexString);
#[cfg(test)]
impl_from_object!(LiteralString, LiteralString);

/// Convert [u8] to Object based on first char,
/// if start with '(' or '<', convert to LiteralString or HexString
/// if start with '/' convert to Name, panic otherwise
#[cfg(test)]
impl From<&[u8]> for Object {
    fn from(value: &[u8]) -> Self {
        use winnow::{Parser as _, error::ContextError};
        assert!(!value.is_empty());
        match value[0] {
            b'(' => Self::LiteralString(LiteralString::new(value)),
            b'<' => crate::parser::hex_string::<_, ContextError>()
                .parse(value)
                .unwrap(),
            b'/' => Self::Name(prescript::name(from_utf8(&value[1..]).unwrap())),
            _ => panic!("invalid object"),
        }
    }
}

impl_from_object!(Stream, Stream);
impl_from_object!(Reference, Reference);
impl_from_object!(Dictionary, Dictionary);

/// Convert &str to Object based on first char,
/// if start with '(' or '<', convert to LiteralString or HexString
/// if start with '/' convert to Name, panic otherwise
#[cfg(test)]
impl From<&str> for Object {
    fn from(value: &str) -> Self {
        value.as_bytes().into()
    }
}

#[cfg(test)]
impl From<u32> for Object {
    fn from(value: u32) -> Self {
        Self::Reference(Reference::new(value, 0))
    }
}

impl From<Vec<Object>> for Object {
    fn from(v: Vec<Object>) -> Self {
        Self::Array(v.into())
    }
}

#[cfg(test)]
mod tests;
