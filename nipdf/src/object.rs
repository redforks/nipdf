//! object mod contains data structure map to low level pdf objects
use ahash::{HashMap, HashMapExt};
use anyhow::Context;
use educe::Educe;
use log::error;
use prescript::Name;
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
pub use stream::*;
pub type Array = Vec<Object>;

#[derive(PartialEq, Debug, Clone, Default, Educe)]
#[educe(Deref, DerefMut)]
pub struct Dictionary(HashMap<Name, Object>);

impl FromIterator<(Name, Object)> for Dictionary {
    fn from_iter<T: IntoIterator<Item = (Name, Object)>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl Dictionary {
    pub fn new() -> Self {
        Self(HashMap::default())
    }

    pub fn get_opt_int_ref(&self, id: Name) -> Result<Option<&i32>, ObjectValueError> {
        self.0
            .get(&id)
            .map_or(Ok(None), |o| o.as_int_ref().map(Some))
    }

    pub fn get_int(&self, id: Name, default: i32) -> Result<i32, ObjectValueError> {
        self.0.get(&id).map_or(Ok(default), |o| o.int())
    }

    pub fn get_bool(&self, id: Name, default: bool) -> Result<bool, ObjectValueError> {
        self.0.get(&id).map_or(Ok(default), |o| o.as_bool())
    }

    pub fn set(&mut self, id: Name, value: impl Into<Object>) {
        self.0.insert(id, value.into());
    }

    pub fn get_name(&self, id: Name) -> Result<Option<&Name>, ObjectValueError> {
        self.0.get(&id).map_or(Ok(None), |o| o.as_name().map(Some))
    }

    pub fn get_name_or(&self, id: Name, default: &'static Name) -> Result<&Name, ObjectValueError> {
        self.0.get(&id).map_or(Ok(default), |o| o.as_name())
    }
}

/// Get type value from Dictionary.
pub trait TypeValueGetter {
    type Value: ?Sized;
    /// Return None if type value is not exist
    fn get<'a>(&self, d: &'a Dictionary) -> Result<Option<&'a Self::Value>, ObjectValueError>;
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

    fn get<'a>(&self, d: &'a Dictionary) -> Result<Option<&'a Self::Value>, ObjectValueError> {
        d.get_opt_int_ref(self.field.clone())
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

    fn get<'a>(&self, d: &'a Dictionary) -> Result<Option<&'a Self::Value>, ObjectValueError> {
        d.get_name(self.field.clone())
    }

    fn field(&self) -> &Name {
        &self.field
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

impl TypeValueCheck<Name> for EqualTypeValueChecker<Name> {
    fn schema_type(&self) -> Cow<str> {
        Cow::Borrowed(self.value.as_ref())
    }

    fn check(&self, v: Option<&Name>) -> bool {
        v.map_or(false, |v| v == &self.value)
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

/// Abstract `ObjectResolver` out, to help
/// `SchemaDict` works without `ObjectResolver`.
/// Some `Dictionary` are known not contains Reference.
pub trait Resolver<'a> {
    fn resolve_reference<'b>(&'b self, v: &'b Object) -> Result<&'b Object, ObjectValueError>;

    fn do_resolve_container_value<'b: 'c, 'c, C: DataContainer>(
        &'b self,
        c: &'c C,
        id: Name,
    ) -> Result<(Option<NonZeroU32>, &'c Object), ObjectValueError>;
}

impl<'a> Resolver<'a> for () {
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
        id: Name,
    ) -> Result<(Option<NonZeroU32>, &'c Object), ObjectValueError> {
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

pub trait PdfObject<'a, 'b, R>
where
    Self: Sized,
    R: Resolver<'a>,
{
    fn new(
        id: Option<NonZeroU32>,
        dict: &'b Dictionary,
        r: &'b R,
    ) -> Result<Self, ObjectValueError>;

    fn checked(
        id: Option<NonZeroU32>,
        dict: &'b Dictionary,
        r: &'b R,
    ) -> Result<Option<Self>, ObjectValueError>;

    fn id(&self) -> Option<NonZeroU32>;

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

impl<'a, 'b, T: TypeValidator, R: 'a + Resolver<'a>> SchemaDict<'b, T, R> {
    fn _opt_resolve_container_value(
        &self,
        id: Name,
    ) -> Result<Option<(Option<NonZeroU32>, &'b Object)>, ObjectValueError> {
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

    fn opt_resolve_value(&self, id: Name) -> Result<Option<&'b Object>, ObjectValueError> {
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

    fn resolve_required_value(
        &self,
        id: Name,
    ) -> Result<(Option<NonZeroU32>, &'b Object), ObjectValueError> {
        self.r
            .do_resolve_container_value(self.d, id.clone())
            .map_err(|e| {
                error!("{}: {}", e, id);
                e
            })
    }

    fn resolve_container_value(&self, id: Name) -> Result<&'b Object, ObjectValueError> {
        self.resolve_required_value(id).map(|(_, o)| o)
    }

    fn opt_get(&self, id: Name) -> Result<Option<&'b Object>, ObjectValueError> {
        self.opt_resolve_value(id)
    }

    pub fn opt_name(&self, id: Name) -> Result<Option<&Name>, ObjectValueError> {
        self.opt_get(id)?
            .map_or(Ok(None), |o| o.as_name().map(Some))
    }

    pub fn required_name(&self, id: Name) -> Result<&Name, ObjectValueError> {
        self.opt_get(id.clone())?
            .ok_or_else(|| ObjectValueError::DictSchemaError(self.t.schema_type(), id.clone()))?
            .as_name()
    }

    pub fn required_int(&self, id: Name) -> Result<i32, ObjectValueError> {
        self.opt_get(id.clone())?
            .ok_or_else(|| ObjectValueError::DictSchemaError(self.t.schema_type(), id.clone()))?
            .int()
    }

    pub fn opt_int(&self, id: Name) -> Result<Option<i32>, ObjectValueError> {
        self.opt_get(id)?.map_or(Ok(None), |o| o.int().map(Some))
    }

    pub fn opt_bool(&self, id: Name) -> Result<Option<bool>, ObjectValueError> {
        self.opt_get(id)?
            .map_or(Ok(None), |o| o.as_bool().map(Some))
    }

    pub fn required_bool(&self, id: Name) -> Result<bool, ObjectValueError> {
        self.opt_get(id.clone())?
            .ok_or_else(|| ObjectValueError::DictSchemaError(self.t.schema_type(), id.clone()))?
            .as_bool()
    }

    pub fn bool_or(&self, id: Name, default: bool) -> Result<bool, ObjectValueError> {
        self.opt_bool(id).map(|b| b.unwrap_or(default))
    }

    pub fn opt_u16(&self, id: Name) -> Result<Option<u16>, ObjectValueError> {
        self.opt_int(id).map(|i| i.map(|i| i as u16))
    }

    pub fn required_u16(&self, id: Name) -> Result<u16, ObjectValueError> {
        self.required_int(id).map(|i| i as u16)
    }

    pub fn opt_u32(&self, id: Name) -> Result<Option<u32>, ObjectValueError> {
        self.opt_int(id).map(|i| {
            // i32 as u32 as a no-op, so it is safe to use `as` operator.
            i.map(|i| i as u32)
        })
    }

    pub fn required_u32(&self, id: Name) -> Result<u32, ObjectValueError> {
        // i32 as u32 as a no-op, so it is safe to use `as` operator.
        self.required_int(id).map(|i| i as u32)
    }

    pub fn u32_or(&self, id: Name, default: u32) -> Result<u32, ObjectValueError> {
        self.opt_u32(id).map(|i| i.unwrap_or(default))
    }

    pub fn opt_u8(&self, id: Name) -> Result<Option<u8>, ObjectValueError> {
        self.opt_int(id).map(|i| i.map(|i| i as u8))
    }

    pub fn required_u8(&self, id: Name) -> Result<u8, ObjectValueError> {
        self.required_int(id).map(|i| i as u8)
    }

    pub fn u8_or(&self, id: Name, default: u8) -> Result<u8, ObjectValueError> {
        self.opt_u8(id).map(|i| i.unwrap_or(default))
    }

    pub fn opt_f32(&self, id: Name) -> Result<Option<f32>, ObjectValueError> {
        self.opt_get(id)?
            .map_or(Ok(None), |o| o.as_number().map(Some))
    }

    pub fn required_f32(&self, id: Name) -> Result<f32, ObjectValueError> {
        self.opt_get(id.clone())?
            .ok_or_else(|| ObjectValueError::DictSchemaError(self.t.schema_type(), id))?
            .as_number()
    }

    pub fn f32_or(&self, id: Name, default: f32) -> Result<f32, ObjectValueError> {
        self.opt_f32(id).map(|i| i.unwrap_or(default))
    }

    pub fn opt_object(&self, id: Name) -> Result<Option<&'b Object>, ObjectValueError> {
        self.opt_get(id)
    }

    pub fn required_object(&self, id: Name) -> Result<&'b Object, ObjectValueError> {
        self.opt_object(id.clone())?
            .ok_or_else(|| ObjectValueError::DictSchemaError(self.t.schema_type(), id))
    }

    /// Return empty vec if not exist, error if not array
    pub fn u32_arr(&self, id: Name) -> Result<Vec<u32>, ObjectValueError> {
        self.opt_arr_map(id, |o| o.int().map(|i| i as u32))
            .map(|o| o.unwrap_or_default())
    }

    /// Return empty vec if not exist, error if not array
    pub fn f32_arr(&self, id: Name) -> Result<Vec<f32>, ObjectValueError> {
        self.opt_arr_map(id, |o| o.as_number())
            .map(|o| o.unwrap_or_default())
    }

    pub fn opt_f32_arr(&self, id: Name) -> Result<Option<Vec<f32>>, ObjectValueError> {
        self.opt_arr_map(id, |o| o.as_number())
            .map(|o| o.unwrap_or_default())
            .map(Some)
    }

    pub fn required_arr_map<V>(
        &self,
        id: Name,
        f: impl Fn(&Object) -> Result<V, ObjectValueError>,
    ) -> Result<Vec<V>, ObjectValueError> {
        self.opt_get(id.clone())?
            .ok_or_else(|| ObjectValueError::DictSchemaError(self.t.schema_type(), id))?
            .as_arr()?
            .iter()
            .map(f)
            .collect()
    }

    pub fn opt_arr_map<V>(
        &self,
        id: Name,
        f: impl Fn(&Object) -> Result<V, ObjectValueError>,
    ) -> Result<Option<Vec<V>>, ObjectValueError> {
        self.opt_get(id)?
            .map_or(Ok(None), |o| o.as_arr().map(Some))?
            .map(|arr| arr.iter().map(f).collect())
            .transpose()
    }

    pub fn opt_arr(&self, id: Name) -> Result<Option<&'b Array>, ObjectValueError> {
        self.opt_get(id)?.map_or(Ok(None), |o| o.as_arr().map(Some))
    }

    pub fn opt_single_or_arr_stream(&self, id: Name) -> Result<Vec<&'b Stream>, ObjectValueError> {
        let resolver = self.resolver();
        match self.resolve_container_value(id)? {
            Object::Array(arr) => arr
                .iter()
                .map(|o| resolver.resolve_reference(o)?.as_stream())
                .collect(),
            o => resolver.resolve_reference(o)?.as_stream().map(|o| vec![o]),
        }
    }

    pub fn opt_dict(&self, id: Name) -> Result<Option<&'b Dictionary>, ObjectValueError> {
        self.opt_get(id)?
            .map_or(Ok(None), |o| o.as_dict().map(Some))
    }

    pub fn required_dict(&self, id: Name) -> Result<&'b Dictionary, ObjectValueError> {
        self.opt_dict(id.clone()).and_then(|o| {
            o.ok_or_else(|| ObjectValueError::DictSchemaError(self.t.schema_type(), id))
        })
    }

    pub fn required_ref(&self, id: Name) -> Result<NonZeroU32, ObjectValueError> {
        self.d
            .get(&id.clone())
            .ok_or_else(|| ObjectValueError::DictSchemaError(self.t.schema_type(), id))?
            .as_ref()
            .map(|r| r.id().id())
    }

    pub fn opt_ref(&self, id: Name) -> Result<Option<NonZeroU32>, ObjectValueError> {
        self.d
            .get(&id)
            .map_or(Ok(None), |o| o.as_ref().map(|r| Some(r.id().id())))
    }

    pub fn ref_id_arr(&self, id: Name) -> Result<Vec<NonZeroU32>, ObjectValueError> {
        self.opt_arr_map(id, |o| o.as_ref().map(|r| r.id().id()))
            .map(|o| o.unwrap_or_default())
    }

    pub fn opt_stream(&self, id: Name) -> Result<Option<&'b Stream>, ObjectValueError> {
        self.opt_get(id)?
            .map_or(Ok(None), |o| o.as_stream().map(Some))
    }

    pub fn opt_str(&self, id: Name) -> Result<Option<&str>, ObjectValueError> {
        self.opt_get(id)?
            .map_or(Ok(None), |o| o.as_string().map(Some))
    }

    pub fn required_str(&self, id: Name) -> Result<&str, ObjectValueError> {
        self.opt_get(id.clone())?
            .ok_or_else(|| ObjectValueError::DictSchemaError(self.t.schema_type(), id))?
            .as_string()
    }

    pub fn opt_resolve_pdf_object<'s, O: PdfObject<'a, 'b, R>>(
        &self,
        id: Name,
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
    pub fn resolve_one_or_more_pdf_object<O>(&self, id: Name) -> Result<Vec<O>, ObjectValueError>
    where
        O: PdfObject<'a, 'b, R>,
    {
        let id_n_obj = self._opt_resolve_container_value(id)?;
        id_n_obj.map_or_else(
            || Ok(vec![]),
            |(id, obj)| match obj {
                Object::Dictionary(d) => Ok(vec![O::new(id, d, self.r)?]),
                Object::Stream(s) => Ok(vec![O::new(id, s.as_dict(), self.r)?]),
                Object::Array(arr) => {
                    let mut res = Vec::with_capacity(arr.len());
                    for obj in arr {
                        let dict = self.r.resolve_reference(obj)?;
                        res.push(O::new(
                            obj.as_ref().ok().map(|id| id.id().id()),
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
    pub fn resolve_pdf_object_array<O>(&self, id: Name) -> Result<Vec<O>, ObjectValueError>
    where
        O: PdfObject<'a, 'b, R>,
    {
        let arr = self.opt_resolve_value(id)?;
        arr.map_or_else(
            || Ok(vec![]),
            |arr| {
                let arr = arr.as_arr()?;
                let mut res = Vec::with_capacity(arr.len());
                for obj in arr {
                    let dict = self.r.resolve_reference(obj)?;
                    res.push(O::new(
                        obj.as_ref().ok().map(|id| id.id().id()),
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
    pub fn resolve_pdf_object_map<O>(&self, id: Name) -> anyhow::Result<HashMap<Name, O>>
    where
        O: PdfObject<'a, 'b, R>,
    {
        let dict = self.opt_resolve_value(id)?;
        dict.map_or_else(
            || Ok(HashMap::default()),
            |dict| {
                let dict = dict.as_dict().context("Value not dict")?;
                let mut res = HashMap::with_capacity(dict.len());
                for k in dict.keys() {
                    let obj: O = self._resolve_pdf_object(dict, k.clone())?;
                    res.insert(k.clone(), obj);
                }
                Ok(res)
            },
        )
    }

    fn _resolve_pdf_object<O: PdfObject<'a, 'b, R>>(
        &self,
        d: &'b Dictionary,
        id: Name,
    ) -> Result<O, ObjectValueError> {
        let (id, obj) = self.r.do_resolve_container_value(d, id)?;
        let obj = match obj {
            Object::Dictionary(d) => d,
            Object::Stream(s) => s.as_dict(),
            _ => return Err(ObjectValueError::UnexpectedType),
        };
        O::new(id, obj, self.r)
    }

    pub fn resolve_pdf_object<O: PdfObject<'a, 'b, R>>(
        &self,
        id: Name,
    ) -> Result<O, ObjectValueError> {
        self._resolve_pdf_object(self.d, id)
    }

    pub fn as_byte_string(&self, id: Name) -> Result<&[u8], ObjectValueError> {
        self.opt_get(id.clone())?
            .ok_or_else(|| ObjectValueError::DictSchemaError(self.t.schema_type(), id))?
            .as_byte_string()
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

    #[cfg(test)]
    pub fn empty() -> Self {
        Self {
            id: NonZeroU32::new(1u32).unwrap(),
            generation: 0,
        }
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
use crate::{file::DataContainer, parser};
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
    ObjectIDNotFound(NonZeroU32),
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
    Stream(Stream),
    Reference(Reference),
}

macro_rules! copy_value_access {
    ($method:ident, $opt_method:ident, $branch:ident, $t:ty) => {
        impl Object {
            /// Return None if value not specific type.
            pub fn $opt_method(&self) -> Option<$t> {
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

        impl TryFrom<Object> for $t {
            type Error = ObjectValueError;

            fn try_from(value: Object) -> Result<Self, Self::Error> {
                value.$method()
            }
        }

        impl From<&Object> for Option<$t> {
            fn from(value: &Object) -> Self {
                value.$opt_method()
            }
        }
    };
}
macro_rules! ref_value_access {
    ($method:ident, $opt_method:ident, $branch:ident, $t:ty) => {
        impl Object {
            /// Return None if value not specific type.
            pub fn $opt_method(&self) -> Option<$t> {
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
    };
}

copy_value_access!(bool, opt_bool, Bool, bool);
copy_value_access!(int, opt_int, Integer, i32);
copy_value_access!(number, opt_number, Number, f32);
ref_value_access!(literal_str, opt_literal_str, LiteralString, &LiteralString);
ref_value_access!(hex_str, opt_hex_str, HexString, &HexString);
copy_value_access!(name, opt_name, Name, Name);
ref_value_access!(dict, opt_dict, Dictionary, &Dictionary);
ref_value_access!(arr, opt_arr, Array, &Array);
ref_value_access!(stream, opt_stream, Stream, &Stream);
copy_value_access!(reference, opt_reference, Reference, Reference);

impl Object {
    pub fn new_ref(id: u32) -> Self {
        Self::Reference(Reference::new_u32(id, 0))
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Return either type value. Panic if value is not either type.
    pub fn either<'a, U, V>(&'a self) -> Either<U, V>
    where
        U: Clone,
        V: Clone + From<Self>,
        Option<U>: From<&'a Self>,
        Option<V>: From<&'a Self>,
    {
        match Option::<U>::from(self) {
            Some(v) => Either::Left(v),
            None => Either::Right(Option::<V>::from(self).expect("not either of")),
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
    pub fn as_name(&self) -> Result<&Name, ObjectValueError> {
        match self {
            Object::Name(n) => Ok(n),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }

    pub fn as_dict(&self) -> Result<&Dictionary, ObjectValueError> {
        match self {
            Object::Dictionary(d) => Ok(d),
            Object::Stream(s) => Ok(s.as_dict()),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }

    pub fn as_stream(&self) -> Result<&Stream, ObjectValueError> {
        match self {
            Object::Stream(s) => Ok(s),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }

    pub fn as_arr(&self) -> Result<&Array, ObjectValueError> {
        match self {
            Object::Array(a) => Ok(a),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }

    pub fn into_arr(self) -> Result<Array, ObjectValueError> {
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
}

impl<const N: usize> TryFrom<&Object> for [f32; N] {
    type Error = ObjectValueError;

    fn try_from(obj: &Object) -> Result<Self, Self::Error> {
        let arr = obj.as_arr()?;
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
#[cfg(feature = "pretty")]
use pretty::RcDoc;

#[cfg(feature = "pretty")]
impl Object {
    pub fn to_doc(&self) -> RcDoc {
        fn name_to_doc(n: &Name) -> RcDoc<'_> {
            RcDoc::text("/").append(RcDoc::text(n.as_ref()))
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

#[cfg(feature = "pretty")]
struct PrettyNumber(f32);

#[cfg(feature = "pretty")]
impl Display for PrettyNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl From<Stream> for Object {
    fn from(value: Stream) -> Self {
        Self::Stream(value)
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
pub struct LiteralString(Box<[u8]>);

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
        let mut result: Vec<u8> = Vec::with_capacity(s.len());
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

        Self(result.into())
    }

    pub fn update(&mut self, f: impl FnOnce(&mut [u8])) {
        f(&mut self.0);
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
    Number(f32),
}

/// Decoded PDF literal string object, enclosing '(' and ')' not included.
#[derive(Eq, PartialEq, Debug, Clone)]
pub struct HexString(Box<[u8]>);

impl HexString {
    pub fn new(s: &[u8]) -> Self {
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

        debug_assert!(s.starts_with(b"<") && s.ends_with(b">"));
        let s = &s[1..s.len() - 1];
        let s = filter_whitespace(s);
        let s = append_zero_if_odd(&s);
        let s = hex::decode(s).unwrap();
        Self(s.into())
    }

    pub fn update(&mut self, f: impl FnOnce(&mut [u8])) {
        f(&mut self.0);
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
impl From<u32> for Object {
    fn from(value: u32) -> Self {
        Self::Reference(Reference::new_u32(value, 0))
    }
}

#[cfg(test)]
mod tests;
