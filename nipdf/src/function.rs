use anyhow::Result as AnyResult;

use nipdf_macro::{pdf_object, TryFromIntObject};
use smallvec::{smallvec, SmallVec};

use crate::object::{Object, ObjectValueError};
#[cfg(test)]
use mockall::automock;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Domain<T = f32> {
    pub start: T,
    pub end: T,
}

impl<T: PartialOrd + Copy> Domain<T> {
    pub fn new(start: T, end: T) -> Self {
        Self { start, end }
    }

    fn clamp(&self, x: T) -> T {
        num::clamp(x, self.start, self.end)
    }
}

/// Default domain is [0, 1]
pub fn default_domain() -> Domain {
    Domain::new(0.0, 1.0)
}

impl<'a> TryFrom<&Object<'a>> for Domain<f32> {
    type Error = ObjectValueError;

    fn try_from(obj: &Object<'a>) -> Result<Self, Self::Error> {
        let arr = obj.as_arr()?;
        if arr.len() != 2 {
            return Err(ObjectValueError::UnexpectedType);
        }
        Ok(Self::new(arr[0].as_number()?, arr[1].as_number()?))
    }
}

impl<'a> TryFrom<&Object<'a>> for Domain<u32> {
    type Error = ObjectValueError;

    fn try_from(obj: &Object<'a>) -> Result<Self, Self::Error> {
        let arr = obj.as_arr()?;
        if arr.len() != 2 {
            return Err(ObjectValueError::UnexpectedType);
        }
        Ok(Self::new(arr[0].as_int()? as u32, arr[1].as_int()? as u32))
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct Domains<T = f32>(pub Vec<Domain<T>>);

impl<'a, T> TryFrom<&Object<'a>> for Domains<T>
where
    Domain<T>: for<'b> TryFrom<&'b Object<'a>, Error = ObjectValueError>,
{
    type Error = ObjectValueError;

    fn try_from(obj: &Object<'a>) -> Result<Self, Self::Error> {
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

pub type FunctionValue = SmallVec<[f32; 4]>;

#[cfg_attr(test, automock)]
pub trait Function {
    fn call(&self, args: &[f32]) -> AnyResult<FunctionValue> {
        let args = self.sigunature().clip_args(args);
        let r = self.inner_call(args)?;
        Ok(self.sigunature().clip_returns(r))
    }

    fn sigunature(&self) -> &Signature;

    /// Called by `self.call()`, args and return value are clipped by signature.
    fn inner_call(&self, args: FunctionValue) -> AnyResult<FunctionValue>;
}

impl Function for Box<dyn Function> {
    fn call(&self, args: &[f32]) -> AnyResult<FunctionValue> {
        self.as_ref().call(args)
    }

    fn sigunature(&self) -> &Signature {
        self.as_ref().sigunature()
    }

    fn inner_call(&self, _args: SmallVec<[f32; 4]>) -> AnyResult<FunctionValue> {
        unreachable!()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, TryFromIntObject)]
pub enum Type {
    Sampled = 0,
    ExponentialInterpolation = 2,
    Stitching = 3,
    PostScriptCalculator = 4,
}

#[pdf_object(())]
pub trait FunctionDictTrait {
    #[try_from]
    fn function_type(&self) -> Type;

    #[try_from]
    fn domain(&self) -> Domains;

    #[try_from]
    fn range(&self) -> Option<Domains>;

    #[self_as]
    fn sampled(&self) -> SampledFunctionDict<'a, 'b>;

    #[self_as]
    fn exponential_interpolation(&self) -> ExponentialInterpolationFunctionDict<'a, 'b>;

    #[self_as]
    fn stitch(&self) -> StitchingFunctionDict<'a, 'b>;
}

impl<'a, 'b> FunctionDict<'a, 'b> {
    fn signature(&self) -> AnyResult<Signature> {
        Ok(Signature {
            domain: self.domain()?,
            range: self.range()?,
        })
    }

    pub fn n_args(&self) -> usize {
        self.domain().unwrap().n()
    }

    pub fn n_returns(&self) -> Option<usize> {
        self.range().unwrap().map(|range| range.n())
    }

    /// Create boxed Function for this Function dict.
    pub fn func(&self) -> AnyResult<Box<dyn Function>> {
        match self.function_type()? {
            Type::Sampled => Ok(Box::new(self.sampled()?.func()?)),
            Type::ExponentialInterpolation => {
                Ok(Box::new(self.exponential_interpolation()?.func()?))
            }
            Type::Stitching => Ok(Box::new(self.stitch()?.func()?)),
            Type::PostScriptCalculator => todo!(),
        }
    }
}

/// Function signature, clip input args and returns.
#[derive(Debug, PartialEq, Clone)]
pub struct Signature {
    domain: Domains,
    range: Option<Domains>,
}

impl Signature {
    pub fn n_args(&self) -> usize {
        self.domain.n()
    }

    pub fn n_returns(&self) -> Option<usize> {
        self.range.as_ref().map(|range| range.n())
    }

    fn clip_args(&self, args: &[f32]) -> SmallVec<[f32; 4]> {
        debug_assert_eq!(args.len(), self.n_args());

        args.iter()
            .zip(self.domain.0.iter())
            .map(|(&arg, domain)| domain.clamp(arg))
            .collect()
    }

    fn clip_returns(&self, returns: FunctionValue) -> FunctionValue {
        let Some(range) = self.range.as_ref() else {
            return returns;
        };
        assert_eq!(returns.len(), range.n());

        returns
            .iter()
            .zip(range.0.iter())
            .map(|(&ret, domain)| domain.clamp(ret))
            .collect()
    }
}

fn f32_zero_arr() -> Vec<f32> {
    vec![0.0]
}

fn f32_one_arr() -> Vec<f32> {
    vec![1.0]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, TryFromIntObject, Default)]
pub enum InterpolationOrder {
    #[default]
    Linear = 1,
    Cubic = 3,
}

#[pdf_object(0i32)]
#[type_field("FunctionType")]
pub trait SampledFunctionDictTrait {
    #[self_as]
    fn function_dict(&self) -> FunctionDict<'a, 'b>;

    fn size(&self) -> Vec<u32>;
    fn bits_per_sample(&self) -> u32;

    #[try_from]
    #[or_default]
    fn order(&self) -> InterpolationOrder;

    #[try_from]
    fn encode(&self) -> Option<Domains>;

    #[try_from]
    fn decode(&self) -> Option<Domains>;
}

/// strut to implement Function trait for SampledFunctionDict,
/// because sampled function need to load sample data from stream.
#[derive(Debug, PartialEq, Clone)]
pub struct SampledFunction {
    signature: Signature,
    encode: Domains,
    decode: Domains,
    size: Vec<u32>,
    samples: Vec<u8>,
}

impl Function for SampledFunction {
    fn inner_call(&self, args: SmallVec<[f32; 4]>) -> AnyResult<FunctionValue> {
        let mut idx = 0;
        for (i, arg) in args.iter().enumerate() {
            let domain = &self.signature.domain.0[i];
            let encode = &self.encode.0[i];
            let arg = (arg - domain.start) / (domain.end - domain.start);
            let arg = arg * (encode.end - encode.start) + encode.start;
            idx += (arg as u32).min(self.size[i] - 1).max(0);
        }

        let n = self.signature.n_returns().unwrap();
        let mut r = smallvec![];
        let decode = &self.decode.0[0];
        for i in 0..n {
            let sample = self.samples[idx as usize * n + i];
            let sample = sample as f32 / 255.0;
            let sample = sample * (decode.end - decode.start) + decode.start;
            r.push(sample);
        }
        Ok(self.signature.clip_returns(r))
    }

    fn sigunature(&self) -> &Signature {
        &self.signature
    }
}

impl<'a, 'b> SampledFunctionDict<'a, 'b> {
    /// Return SampledFunction instance which implements Function trait.
    pub fn func(&self) -> AnyResult<SampledFunction> {
        let f = self.function_dict()?;
        assert_eq!(1, f.n_args(), "todo: support multi args");

        assert_eq!(
            8,
            self.bits_per_sample()?,
            "todo: support bits_per_sample != 8"
        );
        assert_eq!(InterpolationOrder::Linear, self.order()?);
        let size = self.size()?;
        let resolver = self.d.resolver();
        let stream = resolver.resolve(self.id.unwrap())?.as_stream()?;
        let sample_data = stream.decode(resolver)?;
        let signature = f.signature()?;
        assert!(sample_data.len() >= size[0] as usize * signature.n_returns().unwrap());
        Ok(SampledFunction {
            signature,
            encode: self.encode()?.unwrap_or_else(|| {
                Domains(
                    size.iter()
                        .map(|v| Domain::new(0.0, (*v - 1) as f32))
                        .collect(),
                )
            }),
            decode: self.decode()?.unwrap_or_else(|| {
                f.range()
                    .unwrap()
                    .expect("range should exist in sampled function")
            }),
            size: self.size()?,
            samples: sample_data.into_owned(),
        })
    }
}

#[pdf_object(2i32)]
#[type_field("FunctionType")]
pub trait ExponentialInterpolationFunctionDictTrait {
    #[default_fn(f32_zero_arr)]
    fn c0(&self) -> Vec<f32>;

    #[default_fn(f32_one_arr)]
    fn c1(&self) -> Vec<f32>;

    fn n(&self) -> f32;

    #[self_as]
    fn function_dict(&self) -> FunctionDict<'a, 'b>;
}

pub struct ExponentialInterpolationFunction {
    c0: Vec<f32>,
    c1: Vec<f32>,
    n: f32,
    signature: Signature,
}

impl Function for ExponentialInterpolationFunction {
    fn inner_call(&self, args: SmallVec<[f32; 4]>) -> AnyResult<FunctionValue> {
        let x = args[0];
        let c0 = &self.c0;
        let c1 = &self.c1;
        let n = self.n;
        let r = (0..c0.len())
            .map(|i| c0[i] + x.powf(n) * (c1[i] - c0[i]))
            .collect();
        Ok(self.signature.clip_returns(r))
    }

    fn sigunature(&self) -> &Signature {
        &self.signature
    }
}

impl<'a, 'b> ExponentialInterpolationFunctionDict<'a, 'b> {
    pub fn func(&self) -> AnyResult<ExponentialInterpolationFunction> {
        let f = self.function_dict()?;
        let signature = f.signature()?;
        let c0 = self.c0()?;
        let c1 = self.c1()?;
        let n = self.n()?;
        assert_eq!(n.fract(), 0.0);
        Ok(ExponentialInterpolationFunction {
            c0,
            c1,
            n,
            signature,
        })
    }
}

#[pdf_object(3i32)]
#[type_field("FunctionType")]
pub trait StitchingFunctionDictTrait {
    /// Functions, its length is `k`
    #[nested]
    fn functions(&self) -> Vec<FunctionDict<'a, 'b>>;

    /// The number of values shall be `k - 1`
    fn bounds(&self) -> Vec<f32>;

    /// The number of values shall be `k`
    #[try_from]
    fn encode(&self) -> Domains;

    #[self_as]
    fn function_dict(&self) -> FunctionDict<'a, 'b>;
}

impl<'a, 'b> StitchingFunctionDict<'a, 'b> {
    pub fn func(&self) -> AnyResult<StitchingFunction> {
        let functions = self
            .functions()?
            .into_iter()
            .map(|f| f.func())
            .collect::<AnyResult<_>>()?;
        let bounds = self.bounds()?;
        let encode = self.encode()?;
        let f = self.function_dict()?;
        let signature = Signature {
            domain: f.domain()?,
            range: f.range()?,
        };
        Ok(StitchingFunction {
            functions,
            bounds,
            encode,
            signature,
        })
    }
}

pub struct StitchingFunction {
    functions: Vec<Box<dyn Function>>,
    bounds: Vec<f32>,
    encode: Domains,
    signature: Signature,
}

impl StitchingFunction {
    fn find_function(bounds: &[f32], x: f32) -> usize {
        bounds
            .iter()
            .position(|&bound| x < bound)
            .unwrap_or(bounds.len())
    }

    fn sub_domain(domain: &Domain, bounds: &[f32], idx: usize) -> Domain {
        let start = if idx == 0 {
            domain.start
        } else {
            bounds[idx - 1]
        };
        let end = if idx == bounds.len() {
            domain.end
        } else {
            bounds[idx]
        };
        Domain::new(start, end)
    }

    fn interpolation(a: &Domain, b: &Domain, t: f32) -> f32 {
        let a_len = a.end - a.start;
        let b_len = b.end - b.start;
        let t = (t - a.start) / a_len;
        b.start + t * b_len
    }

    fn domains(&self) -> &Domains {
        &self.signature.domain
    }
}

impl Function for StitchingFunction {
    fn inner_call(&self, args: SmallVec<[f32; 4]>) -> AnyResult<FunctionValue> {
        assert_eq!(args.len(), 1);

        let x = args[0];
        let function_idx = Self::find_function(&self.bounds, x);
        let sub_domain = Self::sub_domain(&self.domains().0[0], &self.bounds, function_idx);
        let x = Self::interpolation(&sub_domain, &self.encode.0[function_idx], x);

        let f = &self.functions[function_idx];
        let r = f.call(&[x])?;
        Ok(r)
    }

    fn sigunature(&self) -> &Signature {
        return &self.signature;
    }
}

#[cfg(test)]
mod tests;
