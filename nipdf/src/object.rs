//! object mod contains data structure map to low level pdf objects
use ahash::{HashMap, HashMapExt};
use anyhow::Context;
use educe::Educe;
use log::error;
use paste::paste;
use prescript::Name;
use std::{
    borrow::{Borrow, Cow},
    fmt::{Debug, Display},
    iter::{Peekable, repeat},
    rc::Rc,
    str::from_utf8,
};
use tinyvec::TinyVec;

mod indirect_object;
pub use indirect_object::IndirectObject;
mod stream;
pub use stream::*;
pub type Array = Rc<[Object]>;

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

    pub fn update(&mut self, f: impl FnOnce(&mut HashMap<Name, Object>)) {
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
        d.get(&self.field).map(|v| v.name()).transpose()
    }

    fn field(&self) -> &Name {
        &self.field
    }
}

pub trait TypeValueCheck<V>: Clone + Debug {
    fn schema_type(&self) -> Cow<str>;
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
    fn schema_type(&self) -> Cow<str> {
        Cow::Borrowed(self.value.as_str())
    }

    fn check(&self, v: Option<Name>) -> bool {
        v.map_or(false, |v| v == self.value)
    }
}

impl TypeValueCheck<i32> for EqualTypeValueChecker<i32> {
    fn schema_type(&self) -> Cow<str> {
        Cow::Owned(self.value.to_string())
    }

    fn check(&self, v: Option<i32>) -> bool {
        v.map_or(false, |v| v == self.value)
    }
}

/// impl `TypeValueCheck` return true if value is None, otherwise check value using `inner`.
#[derive(Clone, Debug)]
pub struct OptionTypeValueChecker<Inner: Sized + Clone + Debug>(pub Inner);

impl<Inner: TypeValueCheck<V> + Clone + Debug, V> TypeValueCheck<V>
    for OptionTypeValueChecker<Inner>
{
    fn schema_type(&self) -> Cow<str> {
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
    fn schema_type(&self) -> Cow<str> {
        Cow::Owned(
            self.values
                .iter()
                .map(|v| v.borrow().to_string())
                .collect::<Vec<_>>()
                .join("|"),
        )
    }

    fn check(&self, v: Option<V>) -> bool {
        v.map_or(false, |v| self.values.iter().any(|r| &v == r))
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

/// Abstract `ObjectResolver` out, to help
/// `SchemaDict` works without `ObjectResolver`.
/// Some `Dictionary` are known not contains Reference.
pub trait Resolver {
    fn resolve_reference<'b>(&'b self, v: &'b Object) -> Result<&'b Object, ObjectValueError>;

    fn do_resolve_container_value<'b: 'c, 'c, C: DataContainer>(
        &'b self,
        c: &'c C,
        id: &Name,
    ) -> Result<(Option<RuntimeObjectId>, &'c Object), ObjectValueError>;
}

impl Resolver for () {
    fn resolve_reference<'b>(&'b self, v: &'b Object) -> Result<&'b Object, ObjectValueError> {
        debug_assert!(
            !matches!(v, Object::Reference(_)),
            "Cannot resolve id in current SchemaDict"
        );
        Ok(v)
    }

    fn do_resolve_container_value<'b: 'c, 'c, C: DataContainer>(
        &'b self,
        c: &'c C,
        id: &Name,
    ) -> Result<(Option<RuntimeObjectId>, &'c Object), ObjectValueError> {
        c.get_value(id)
            .map(|o| {
                debug_assert!(
                    !matches!(o, Object::Reference(_)),
                    "Cannot resolve id in current SchemaDict"
                );
                (None, o)
            })
            .ok_or(ObjectValueError::DictKeyNotFound)
    }
}

pub trait PdfObject<'b, R>
where
    Self: Sized,
    R: Resolver,
{
    fn new(
        id: Option<RuntimeObjectId>,
        dict: &'b Dictionary,
        r: &'b R,
    ) -> Result<Self, ObjectValueError>;

    fn checked(
        id: Option<RuntimeObjectId>,
        dict: &'b Dictionary,
        r: &'b R,
    ) -> Result<Option<Self>, ObjectValueError>;

    fn id(&self) -> Option<RuntimeObjectId>;

    fn dict(&self) -> &Dictionary;

    fn resolver(&self) -> &'b R;
}

#[derive(Educe)]
#[educe(Debug, Clone)]
pub struct SchemaDict<'b, T: Clone + Debug, R> {
    t: T,
    d: &'b Dictionary,
    #[educe(Debug(ignore))]
    r: &'b R,
}

impl<'b, T: TypeValidator, R> SchemaDict<'b, T, R> {
    pub fn new(d: &'b Dictionary, r: &'b R, t: T) -> Result<Self, ObjectValueError> {
        t.valid(d)?;
        Ok(Self { t, d, r })
    }

    pub fn from(d: &'b Dictionary, r: &'b R, t: T) -> Result<Option<Self>, ObjectValueError> {
        if t.check(d)? {
            Ok(Some(Self { t, d, r }))
        } else {
            Ok(None)
        }
    }

    pub fn dict(&self) -> &'b Dictionary {
        self.d
    }

    pub fn resolver(&self) -> &'b R {
        self.r
    }
}

macro_rules! schema_access {
    ($method:ident, $t:ty) => {
        paste! {
            pub fn $method(&self, id: &Name) -> Result<$t, ObjectValueError> {
                self.required::<$t>(id)
            }

            pub fn [<opt_ $method>](&self, id: &Name) -> Result<Option<$t>, ObjectValueError> {
                self.opt::<$t>(id)
            }

            pub fn [<$method _or>](&self, id: &Name, v: $t) -> Result<$t, ObjectValueError> {
                self.or(id, v)
            }
        }
    };
}

impl<'a, 'b, T: TypeValidator, R: 'a + Resolver> SchemaDict<'b, T, R> {
    schema_access!(bool, bool);

    schema_access!(int, i32);

    schema_access!(name, Name);

    pub fn required<V: for<'d> TryFrom<&'d Object, Error = ObjectValueError>>(
        &self,
        id: &Name,
    ) -> Result<V, ObjectValueError> {
        let v = self.dict().get(id);
        v.map_or(Err(ObjectValueError::DictKeyNotFound), |v| {
            let v = self.r.resolve_reference(v)?;
            v.try_into()
        })
    }

    pub fn opt<V: for<'d> TryFrom<&'d Object, Error = ObjectValueError>>(
        &self,
        id: &Name,
    ) -> Result<Option<V>, ObjectValueError> {
        let v = self.dict().get(id);
        v.map(|v| {
            let v = self.r.resolve_reference(v)?;
            v.try_into()
        })
        .transpose()
    }

    pub fn or<V: for<'d> TryFrom<&'d Object, Error = ObjectValueError>>(
        &self,
        id: &Name,
        default: V,
    ) -> Result<V, ObjectValueError> {
        self.opt(id).map(|o| o.unwrap_or(default))
    }

    fn _opt_resolve_container_value(
        &self,
        id: &Name,
    ) -> Result<Option<(Option<RuntimeObjectId>, &'b Object)>, ObjectValueError> {
        self.r
            .do_resolve_container_value(self.d, id)
            .map(Some)
            .or_else(|e| match e {
                ObjectValueError::ObjectIDNotFound(_) | ObjectValueError::DictKeyNotFound => {
                    Ok(None)
                }
                _ => Err(e),
            })
    }

    fn opt_resolve_value(&self, id: &Name) -> Result<Option<&'b Object>, ObjectValueError> {
        self.r
            .do_resolve_container_value(self.d, id)
            .map(|(_, o)| o)
            .map(Some)
            .or_else(|e| match e {
                ObjectValueError::ObjectIDNotFound(_) | ObjectValueError::DictKeyNotFound => {
                    Ok(None)
                }
                _ => Err(e),
            })
    }

    fn opt_get(&self, id: &Name) -> Result<Option<&'b Object>, ObjectValueError> {
        self.opt_resolve_value(id)
    }

    pub fn opt_u16(&self, id: &Name) -> Result<Option<u16>, ObjectValueError> {
        self.opt_int(id).map(|i| i.map(|i| i.try_into().unwrap()))
    }

    pub fn required_u16(&self, id: &Name) -> Result<u16, ObjectValueError> {
        self.int(id).map(|i| i.try_into().unwrap())
    }

    pub fn opt_u32(&self, id: &Name) -> Result<Option<u32>, ObjectValueError> {
        self.opt_int(id).map(|i| {
            // i32 as u32 as a no-op, so it is safe to use `as` operator.
            // truncate is expected here, so allow it.
            #[allow(clippy::cast_possible_truncation)]
            i.map(|i| i as u32)
        })
    }

    pub fn required_u32(&self, id: &Name) -> Result<u32, ObjectValueError> {
        // i32 as u32 as a no-op, so it is safe to use `as` operator.
        self.int(id).map(|i| i as u32)
    }

    pub fn u32_or(&self, id: &Name, default: u32) -> Result<u32, ObjectValueError> {
        self.opt_u32(id).map(|i| i.unwrap_or(default))
    }

    pub fn opt_u8(&self, id: &Name) -> Result<Option<u8>, ObjectValueError> {
        self.opt_int(id).map(|i| i.map(|i| i.try_into().unwrap()))
    }

    pub fn required_u8(&self, id: &Name) -> Result<u8, ObjectValueError> {
        self.int(id).map(|i| i.try_into().unwrap())
    }

    pub fn u8_or(&self, id: &Name, default: u8) -> Result<u8, ObjectValueError> {
        self.opt_u8(id).map(|i| i.unwrap_or(default))
    }

    pub fn opt_f32(&self, id: &Name) -> Result<Option<f32>, ObjectValueError> {
        self.opt_get(id)?
            .map_or(Ok(None), |o| o.as_number().map(Some))
    }

    pub fn required_f32(&self, id: &Name) -> Result<f32, ObjectValueError> {
        self.opt_get(id)?
            .ok_or_else(|| ObjectValueError::DictSchemaError(self.t.schema_type(), id.clone()))?
            .as_number()
    }

    pub fn f32_or(&self, id: &Name, default: f32) -> Result<f32, ObjectValueError> {
        self.opt_f32(id).map(|i| i.unwrap_or(default))
    }

    pub fn opt_object(&self, id: &Name) -> Result<Option<&'b Object>, ObjectValueError> {
        self.opt_get(id)
    }

    pub fn required_object(&self, id: &Name) -> Result<&'b Object, ObjectValueError> {
        self.opt_object(id)?
            .ok_or_else(|| ObjectValueError::DictSchemaError(self.t.schema_type(), id.clone()))
    }

    /// Return empty vec if not exist, error if not array
    pub fn u32_arr(&self, id: &Name) -> Result<Vec<u32>, ObjectValueError> {
        self.opt_arr_map(id, |o| o.as_int().map(|i| i as u32))
            .map(|o| o.unwrap_or_default())
    }

    /// Return empty vec if not exist, error if not array
    pub fn f32_arr(&self, id: &Name) -> Result<Vec<f32>, ObjectValueError> {
        self.opt_arr_map(id, |o| o.as_number())
            .map(|o| o.unwrap_or_default())
    }

    pub fn opt_f32_arr(&self, id: &Name) -> Result<Option<Vec<f32>>, ObjectValueError> {
        self.opt_arr_map(id, |o| o.as_number())
            .map(|o| o.unwrap_or_default())
            .map(Some)
    }

    pub fn required_arr_map<V>(
        &self,
        id: &Name,
        f: impl Fn(&Object) -> Result<V, ObjectValueError>,
    ) -> Result<Vec<V>, ObjectValueError> {
        self.opt_get(id)?
            .ok_or_else(|| ObjectValueError::DictSchemaError(self.t.schema_type(), id.clone()))?
            .arr()?
            .iter()
            .map(f)
            .collect()
    }

    pub fn opt_arr_map<V>(
        &self,
        id: &Name,
        f: impl Fn(&Object) -> Result<V, ObjectValueError>,
    ) -> Result<Option<Vec<V>>, ObjectValueError> {
        self.opt_get(id)?
            .map_or(Ok(None), |o| o.arr().map(Some))?
            .map(|arr| arr.iter().map(f).collect())
            .transpose()
    }

    pub fn opt_arr(&self, id: &Name) -> Result<Option<&'b Array>, ObjectValueError> {
        self.opt_get(id)?.map_or(Ok(None), |o| o.arr().map(Some))
    }

    pub fn opt_single_or_arr_stream(&self, id: &Name) -> Result<Vec<&'b Stream>, ObjectValueError> {
        let resolver = self.resolver();
        match self._opt_resolve_container_value(id)? {
            Some((_, Object::Array(arr))) => arr
                .iter()
                .map(|o| resolver.resolve_reference(o)?.stream())
                .collect(),
            None => Ok(vec![]),
            Some((_, o)) => resolver.resolve_reference(o)?.stream().map(|o| vec![o]),
        }
    }

    pub fn opt_dict(&self, id: &Name) -> Result<Option<&'b Dictionary>, ObjectValueError> {
        self.opt_get(id)?
            .map_or(Ok(None), |o| o.as_dict().map(Some))
    }

    pub fn required_dict(&self, id: &Name) -> Result<&'b Dictionary, ObjectValueError> {
        self.opt_dict(id).and_then(|o| {
            o.ok_or_else(|| ObjectValueError::DictSchemaError(self.t.schema_type(), id.clone()))
        })
    }

    pub fn required_ref(&self, id: &Name) -> Result<RuntimeObjectId, ObjectValueError> {
        self.d
            .get(id)
            .ok_or_else(|| ObjectValueError::DictSchemaError(self.t.schema_type(), id.clone()))?
            .reference()
            .map(|r| r.id().id())
    }

    pub fn opt_ref(&self, id: &Name) -> Result<Option<RuntimeObjectId>, ObjectValueError> {
        self.d
            .get(id)
            .map_or(Ok(None), |o| o.reference().map(|r| Some(r.id().id())))
    }

    pub fn ref_id_arr(&self, id: &Name) -> Result<Vec<RuntimeObjectId>, ObjectValueError> {
        self.opt_arr_map(id, |o| o.reference().map(|r| r.id().id()))
            .map(|o| o.unwrap_or_default())
    }

    pub fn stream_dict(&self, id: &Name) -> Result<HashMap<Name, Stream>, ObjectValueError> {
        let resolver = self.resolver();
        let mut res = HashMap::new();
        let (_, v) = self
            ._opt_resolve_container_value(id)?
            .ok_or(ObjectValueError::DictKeyNotFound)?;
        for (k, v) in v.dict()?.iter() {
            let v = resolver.resolve_reference(v)?;
            res.insert(k.clone(), v.stream()?.clone());
        }
        Ok(res)
    }

    pub fn opt_stream(&self, id: &Name) -> Result<Option<&'b Stream>, ObjectValueError> {
        self.opt_get(id)?.map_or(Ok(None), |o| o.stream().map(Some))
    }

    pub fn opt_str(&self, id: &Name) -> Result<Option<&str>, ObjectValueError> {
        self.opt_get(id)?
            .map_or(Ok(None), |o| o.as_string().map(Some))
    }

    pub fn required_str(&self, id: &Name) -> Result<&str, ObjectValueError> {
        self.opt_get(id)?
            .ok_or_else(|| ObjectValueError::DictSchemaError(self.t.schema_type(), id.clone()))?
            .as_string()
    }

    pub fn opt_resolve_pdf_object<'s, O: PdfObject<'b, R>>(
        &self,
        id: &Name,
    ) -> Result<Option<O>, ObjectValueError> {
        if let Some((id, obj)) = self._opt_resolve_container_value(id)? {
            match obj {
                Object::Dictionary(d) => Ok(Some(O::new(id, d, self.r)?)),
                Object::Stream(s) => Ok(Some(O::new(id, s.as_dict(), self.r)?)),
                _ => Err(ObjectValueError::UnexpectedType),
            }
        } else {
            Ok(None)
        }
    }

    /// Resolve pdf_object from container, if its end value is dictionary, return with one element
    /// vec. If its end value is array, return all elements in array.
    /// If value not exist, return empty vector.
    pub fn resolve_one_or_more_pdf_object<O>(&self, id: &Name) -> Result<Vec<O>, ObjectValueError>
    where
        O: PdfObject<'b, R>,
    {
        let id_n_obj = self._opt_resolve_container_value(id)?;
        id_n_obj.map_or_else(
            || Ok(vec![]),
            |(id, obj)| match obj {
                Object::Dictionary(d) => Ok(vec![O::new(id, d, self.r)?]),
                Object::Stream(s) => Ok(vec![O::new(id, s.as_dict(), self.r)?]),
                Object::Array(arr) => {
                    let mut res = Vec::with_capacity(arr.len());
                    for obj in arr.iter() {
                        let dict = self.r.resolve_reference(obj)?;
                        res.push(O::new(
                            obj.reference().ok().map(|id| id.id().id()),
                            dict.as_dict()?,
                            self.r,
                        )?);
                    }
                    Ok(res)
                }
                _ => Err(ObjectValueError::UnexpectedType),
            },
        )
    }

    /// Resolve root pdf_objects from data container `c` with key `k`, if value is reference,
    /// resolve it recursively. Return empty vector if object is not found.
    /// The raw value should be an array of references.
    pub fn resolve_pdf_object_array<O>(&self, id: &Name) -> Result<Vec<O>, ObjectValueError>
    where
        O: PdfObject<'b, R>,
    {
        let arr = self.opt_resolve_value(id)?;
        arr.map_or_else(
            || Ok(vec![]),
            |arr| {
                let arr = arr.arr()?;
                let mut res = Vec::with_capacity(arr.len());
                for obj in arr.iter() {
                    let dict = self.r.resolve_reference(obj)?;
                    res.push(O::new(
                        obj.reference().ok().map(|id| id.id().id()),
                        dict.as_dict()?,
                        self.r,
                    )?);
                }
                Ok(res)
            },
        )
    }

    /// Resolve pdf object from data container `c` with key `k`, if value is reference,
    /// resolve it recursively. Return empty Map if object is not found.
    /// The raw value should be a dictionary, that key is Name and value is Dictionary.
    pub fn resolve_pdf_object_map<O>(&self, id: &Name) -> anyhow::Result<HashMap<Name, O>>
    where
        O: PdfObject<'b, R>,
    {
        let dict = self.opt_resolve_value(id)?;
        dict.map_or_else(
            || Ok(HashMap::default()),
            |dict| {
                let dict = dict.as_dict().context("Value not dict")?;
                let mut res = HashMap::with_capacity(dict.len());
                for k in dict.keys() {
                    let obj: O = self._resolve_pdf_object(dict, k)?;
                    res.insert(k.clone(), obj);
                }
                Ok(res)
            },
        )
    }

    fn _resolve_pdf_object<O: PdfObject<'b, R>>(
        &self,
        d: &'b Dictionary,
        id: &Name,
    ) -> Result<O, ObjectValueError> {
        let (id, obj) = self.r.do_resolve_container_value(d, id)?;
        let obj = match obj {
            Object::Dictionary(d) => d,
            Object::Stream(s) => s.as_dict(),
            _ => return Err(ObjectValueError::UnexpectedType),
        };
        O::new(id, obj, self.r)
    }

    pub fn resolve_pdf_object<O: PdfObject<'b, R>>(
        &self,
        id: &Name,
    ) -> Result<O, ObjectValueError> {
        self._resolve_pdf_object(self.d, id)
    }

    pub fn as_byte_string(&self, id: &Name) -> Result<&[u8], ObjectValueError> {
        self.opt_get(id)?
            .ok_or_else(|| ObjectValueError::DictSchemaError(self.t.schema_type(), id.clone()))?
            .as_byte_string()
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
pub use xref::{Entry as XRefEntry, Section as XRefSection, *};

mod frame;
use crate::{file::DataContainer, graphics::trans::ThousandthsOfText, parser};
pub use frame::*;

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
    #[error("Stream length not defined")]
    StreamLengthNotDefined,
    #[error("Object not found by id {0}")]
    ObjectIDNotFound(RuntimeObjectId),
    #[error("Parse error: {0}")]
    ParseError(String),
    #[error("Unexpected dict schema type, schema: {0}")]
    DictSchemaUnExpectedType(String),
    #[error("Dict schema error, schema: {0}, key: {1}")]
    DictSchemaError(String, Name),
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
    Stream(Rc<Stream>),
    Reference(Reference),
}

macro_rules! copy_value_access {
    ($method:ident, $branch:ident, $t:ty) => {
        impl Object {
            paste! {
                /// Return None if value not specific type.
                pub fn [<opt_ $method>](&self) -> Option<$t> {
                    match self {
                        Self::$branch(v) => Some(v.clone()),
                        _ => None,
                    }
                }

                /// Return `ObjectValueError::UnexpectedType` if value not expected type.
                pub fn $method(&self) -> Result<$t, ObjectValueError> {
                    match self {
                        Self::$branch(v) => Ok(v.clone()),
                        _ => Err(ObjectValueError::UnexpectedType),
                    }
                }
            }
        }

        impl TryFrom<&Object> for $t {
            type Error = ObjectValueError;

            fn try_from(value: &Object) -> Result<Self, Self::Error> {
                value.$method()
            }
        }

        impl From<&Object> for Option<$t> {
            paste! {
                fn from(value: &Object) -> Self {
                    value.[<opt_ $method>]()
                }
            }
        }
    };
}
macro_rules! ref_value_access {
    ($method:ident, $branch:ident, $t:ty) => {
        impl Object {
            paste! {
                /// Return None if value not specific type.
                pub fn [<opt_ $method>](&self) -> Option<$t> {
                    match self {
                        Self::$branch(v) => Some(&v),
                        _ => None,
                    }
                }

                /// Return `ObjectValueError::UnexpectedType` if value not expected type.
                pub fn $method(&self) -> Result<$t, ObjectValueError> {
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
copy_value_access!(int, Integer, i32);
copy_value_access!(real, Number, f32);
ref_value_access!(literal_str, LiteralString, &LiteralString);
ref_value_access!(hex_str, HexString, &HexString);
copy_value_access!(name, Name, Name);
ref_value_access!(dict, Dictionary, &Dictionary);
ref_value_access!(arr, Array, &Array);
ref_value_access!(stream, Stream, &Stream);
copy_value_access!(reference, Reference, Reference);

impl From<Vec<Object>> for Object {
    fn from(v: Vec<Object>) -> Self {
        Self::Array(v.into())
    }
}

impl Object {
    pub fn new_ref(id: u32) -> Self {
        Self::Reference(Reference::new(id, 0))
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Return either type value. Panic if value is not either type.
    pub fn either<'a, U, V>(&'a self) -> Result<Either<U, V>, ObjectValueError>
    where
        U: Clone + TryFrom<&'a Self, Error = ObjectValueError>,
        V: Clone + TryFrom<&'a Self, Error = ObjectValueError>,
    {
        match U::try_from(self) {
            Ok(u) => Ok(Either::Left(u)),
            Err(_) => V::try_from(self).map(Either::Right),
        }
    }

    /// Get number as i32, if value is f32, convert to i32, error otherwise.
    pub fn as_int(&self) -> Result<i32, ObjectValueError> {
        self.either::<f32, i32>()
            .map(|v| v.map_either(|v| v.to_i32().unwrap(), |v| v).into_inner())
    }

    pub fn as_number(&self) -> Result<f32, ObjectValueError> {
        self.either::<f32, i32>()
            .map(|v| v.map_either(|v| v, |v| v as f32).into_inner())
    }

    pub fn as_dict(&self) -> Result<&Dictionary, ObjectValueError> {
        match self {
            Object::Dictionary(d) => Ok(d),
            Object::Stream(s) => Ok(s.as_dict()),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }

    pub fn into_arr(self) -> Result<Array, ObjectValueError> {
        match self {
            Object::Array(a) => Ok(a),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }

    pub fn as_text_string(&self) -> Result<&str, ObjectValueError> {
        match self {
            Object::LiteralString(s) => Ok(s.as_str()),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }

    /// Return decoded string from LiteralString or HexString
    pub fn as_string(&self) -> Result<&str, ObjectValueError> {
        match self {
            Object::LiteralString(s) => Ok(s.as_str()),
            Object::HexString(s) => Ok(s.as_str()),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }

    /// Decode Literal string and hex string into bytes.
    pub fn as_byte_string(&self) -> Result<&[u8], ObjectValueError> {
        match self {
            Object::LiteralString(s) => Ok(s.as_bytes()),
            Object::HexString(s) => Ok(s.as_bytes()),
            _ => Err(ObjectValueError::UnexpectedType),
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
    pub fn update_array_items(arr: &mut Array, mut f: impl FnMut(&mut Object)) {
        match Rc::get_mut(arr) {
            Some(r) => {
                for o in r.iter_mut() {
                    f(o);
                }
            }
            None => {
                let mut ar: Vec<_> = arr.iter().cloned().collect();
                for o in ar.iter_mut() {
                    f(o);
                }
                let ar: Rc<[Object]> = ar.into();
                *arr = ar;
            }
        }
    }
}

impl<const N: usize> TryFrom<&Object> for [f32; N] {
    type Error = ObjectValueError;

    fn try_from(obj: &Object) -> Result<Self, Self::Error> {
        let arr = obj.arr()?;
        if arr.len() != N {
            return Err(ObjectValueError::UnexpectedType);
        }
        let mut r = [0.0; N];
        for (i, v) in arr.iter().enumerate() {
            r[i] = v.as_number()?;
        }
        Ok(r)
    }
}

use either::Either;
use euclid::Length;
use num_traits::ToPrimitive;
use pretty::RcDoc;
use static_assertions::assert_eq_size;

impl Object {
    pub fn to_doc(&self) -> RcDoc {
        fn name_to_doc(n: &Name) -> RcDoc<'_> {
            RcDoc::text("/").append(RcDoc::text(n.as_str()))
        }

        fn dict_to_doc(d: &Dictionary) -> RcDoc<'_> {
            let mut keys = d.keys().collect::<Vec<_>>();
            keys.sort();
            RcDoc::text("<<")
                .append(
                    RcDoc::intersperse(
                        keys.into_iter().map(|k| {
                            name_to_doc(k)
                                .append(RcDoc::space())
                                .append(d.get(k).unwrap().to_doc())
                        }),
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
            Object::LiteralString(s) => RcDoc::text(
                from_utf8(&s.0)
                    .map(|s| format!("({})", s))
                    .unwrap_or_else(|_| format!("0x{}", hex::encode(s.as_bytes()))),
            ),
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
            Object::Reference(r) => RcDoc::as_string(r.id().id())
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

impl From<Stream> for Object {
    fn from(value: Stream) -> Self {
        Self::Stream(Rc::new(value))
    }
}

impl From<Array> for Object {
    fn from(value: Array) -> Self {
        Self::Array(value)
    }
}

impl From<Reference> for Object {
    fn from(value: Reference) -> Self {
        Self::Reference(value)
    }
}

impl From<Dictionary> for Object {
    fn from(value: Dictionary) -> Self {
        Self::Dictionary(value)
    }
}

impl From<Name> for Object {
    fn from(value: Name) -> Self {
        Self::Name(value)
    }
}

/// Convert [u8] to Object based on first char,
/// if start with '(' or '<', convert to LiteralString or HexString
/// if start with '/' convert to Name, panic otherwise
#[cfg(test)]
impl<'a> From<&'a [u8]> for Object {
    fn from(value: &'a [u8]) -> Self {
        assert!(!value.is_empty());
        match value[0] {
            b'(' => Self::LiteralString(LiteralString::new(value)),
            b'<' => Self::HexString(HexString::new(value)),
            b'/' => Self::Name(prescript::name(from_utf8(&value[1..]).unwrap())),
            _ => panic!("invalid object"),
        }
    }
}

/// Convert &str to Object based on first char,
/// if start with '(' or '<', convert to LiteralString or HexString
/// if start with '/' convert to Name, panic otherwise
#[cfg(test)]
impl<'a> From<&'a str> for Object {
    fn from(value: &'a str) -> Self {
        value.as_bytes().into()
    }
}

impl From<f32> for Object {
    fn from(value: f32) -> Self {
        Self::Number(value)
    }
}

impl From<i32> for Object {
    fn from(value: i32) -> Self {
        Self::Integer(value)
    }
}

impl From<bool> for Object {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub struct LiteralString(pub(crate) InnerString);

impl LiteralString {
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

    pub fn as_str(&self) -> &str {
        from_utf8(&self.0).unwrap()
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl From<LiteralString> for Object {
    fn from(value: LiteralString) -> Self {
        Self::LiteralString(value)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TextString {
    Text(LiteralString),
    // maybe CID font
    HexText(HexString),
}

impl TextString {
    pub fn to_bytes(&self) -> Result<&[u8], ObjectValueError> {
        match self {
            TextString::Text(s) => Ok(s.as_bytes()),
            TextString::HexText(s) => Ok(s.as_bytes()),
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
    pub fn new(s: &[u8]) -> Self {
        fn filter_whitespace(s: &mut InnerString) {
            s.retain(|b| !b.is_ascii_whitespace());
        }

        fn append_zero_if_odd(s: &mut InnerString) {
            if s.len() % 2 != 0 {
                s.push(b'0');
            }
        }

        debug_assert!(s.starts_with(b"<") && s.ends_with(b">"));
        let mut s: InnerString = s[1..s.len() - 1].into();
        filter_whitespace(&mut s);
        append_zero_if_odd(&mut s);
        assert!(s.len() % 2 == 0);
        let mut r: InnerString = repeat(0u8).take(s.len() / 2).collect();
        hex::decode_to_slice(s, r.as_mut_slice()).unwrap();
        Self(r)
    }

    pub fn update(&mut self, f: impl FnOnce(&mut [u8])) {
        f(self.0.as_mut_slice());
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_ref()
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(self.as_bytes()).unwrap()
    }
}

impl From<HexString> for Object {
    fn from(value: HexString) -> Self {
        Self::HexString(value)
    }
}

#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub struct Reference(ObjectId);

impl Reference {
    pub fn new(id: impl Into<RuntimeObjectId>, generation: u16) -> Self {
        Self(ObjectId::new(id, generation))
    }

    pub fn id(&self) -> ObjectId {
        self.0
    }
}

#[cfg(test)]
impl From<u32> for Object {
    fn from(value: u32) -> Self {
        Self::Reference(Reference::new(value, 0))
    }
}

#[cfg(test)]
mod tests;
