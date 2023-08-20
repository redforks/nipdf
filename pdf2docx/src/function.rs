use std::ops::RangeInclusive;

use pdf2docx_macro::{pdf_object, TryFromIntObject};

use crate::file::ObjectResolver;
use crate::object::{Dictionary, Object, ObjectValueError, SchemaDict};

pub type Domain = RangeInclusive<f32>;

/// Default domain is [0, 1]
pub fn default_domain() -> Domain {
    0.0..=1.0
}

impl<'a> TryFrom<&Object<'a>> for Domain {
    type Error = ObjectValueError;

    fn try_from(obj: &Object) -> Result<Self, Self::Error> {
        let arr = obj.as_arr()?;
        if arr.len() != 2 {
            return Err(ObjectValueError::UnexpectedType);
        }
        Ok(Self::new(arr[0].as_number()?, arr[1].as_number()?))
    }
}

pub(crate) struct Domains(pub Vec<Domain>);

impl<'a> TryFrom<&Object<'a>> for Domains {
    type Error = ObjectValueError;

    fn try_from(obj: &Object) -> Result<Self, Self::Error> {
        let arr = obj.as_arr()?;
        let mut domains = Vec::with_capacity(arr.len() / 2);
        assert!(arr.len() % 2 == 0);
        arr.chunks_exact(2)
            .map(|chunk| Domain::try_from(&Object::Array(chunk.to_vec())))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .for_each(|domain| domains.push(domain));
        Ok(Self(domains))
    }
}

impl Domains {
    /// Function input argument count
    pub fn n(&self) -> usize {
        self.0.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, TryFromIntObject)]
pub(crate) enum Type {
    Sampled = 0,
    ExponentialInterpolation = 2,
    Stitching = 3,
    PostScriptCalculator = 4,
}

#[pdf_object(())]
pub(crate) trait FunctionDictTrait {
    #[try_from]
    fn function_type(&self) -> Type;

    #[try_from]
    fn domain(&self) -> Domains;

    #[try_from]
    fn range(&self) -> Option<Domains>;
}
