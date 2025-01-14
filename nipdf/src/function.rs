use crate::{
    Result,
    object::{Object, ObjectValueError},
};
use educe::Educe;
#[cfg(test)]
use mockall::automock;
use nipdf_macro::{TryFromIntObject, pdf_object};
use num_traits::ToPrimitive;
use prescript::PdfFunc;
use snafu::{OptionExt as _, ResultExt as _, ensure_whatever, whatever};
use tinyvec::{TinyVec, tiny_vec};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Domain<T = f32> {
    pub start: T,
    pub end: T,
}

impl<T: PartialOrd + Copy> Domain<T> {
    pub fn new(start: T, end: T) -> Self {
        Self { start, end }
    }

    pub fn clamp(&self, x: T) -> T {
        num_traits::clamp(x, self.start, self.end)
    }

    pub fn is_zero(&self) -> bool {
        self.start == self.end
    }
}

/// Default domain is [0, 1]
pub fn default_domain() -> Domain {
    Domain::new(0.0, 1.0)
}

impl TryFrom<&Object> for Domain<f32> {
    type Error = ObjectValueError;

    fn try_from(obj: &Object) -> Result<Self, Self::Error> {
        let arr = obj.arr()?;
        if arr.len() != 2 {
            return Err(ObjectValueError::UnexpectedType);
        }
        Ok(Self::new(arr[0].as_number()?, arr[1].as_number()?))
    }
}

impl TryFrom<&Object> for Domain<u32> {
    type Error = ObjectValueError;

    fn try_from(obj: &Object) -> Result<Self, Self::Error> {
        let arr = obj.arr()?;
        if arr.len() != 2 {
            return Err(ObjectValueError::UnexpectedType);
        }
        Ok(Self::new(arr[0].int()? as u32, arr[1].int()? as u32))
    }
}

#[derive(Debug, PartialEq, Clone, Educe)]
#[educe(Deref)]
pub struct Domains<T = f32>(pub Vec<Domain<T>>);

impl TryFrom<&Object> for Domains<f32> {
    type Error = ObjectValueError;

    fn try_from(obj: &Object) -> Result<Self, Self::Error> {
        let arr = obj.arr()?;
        ensure_whatever!(arr.len() % 2 == 0, "even number of elements expected");
        let mut domains = Vec::with_capacity(arr.len() / 2);
        arr.chunks_exact(2)
            .map(|chunk| {
                Ok::<_, ObjectValueError>(Domain::new(chunk[0].as_number()?, chunk[1].as_number()?))
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .for_each(|domain| domains.push(domain));
        Ok(Self(domains))
    }
}

impl TryFrom<&Object> for Domains<u32> {
    type Error = ObjectValueError;

    fn try_from(obj: &Object) -> Result<Self, Self::Error> {
        let arr = obj.arr()?;
        let mut domains = Vec::with_capacity(arr.len() / 2);
        ensure_whatever!(arr.len() % 2 == 0, "even number of elements expected");
        arr.chunks_exact(2)
            .map(|chunk| {
                Ok::<_, ObjectValueError>(Domain::new(
                    chunk[0].int()? as u32,
                    chunk[1].int()? as u32,
                ))
            })
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

pub type FunctionValue = TinyVec<[f32; 4]>;

trait InnerFunction {
    type Signature: Signature + std::fmt::Debug;

    fn do_call(&self, args: &[f32]) -> Result<FunctionValue> {
        let args = self.signature().clip_args(args);
        let r = self.inner_call(args)?;
        for v in &r {
            ensure_whatever!(!v.is_nan(), "NaN value");
        }
        Ok(self.signature().clip_returns(r))
    }

    fn signature(&self) -> &Self::Signature;

    /// Called by `self.call()`, args and return value are clipped by signature.
    fn inner_call(&self, args: FunctionValue) -> Result<FunctionValue>;
}

#[cfg_attr(test, automock)]
pub trait Function {
    fn call(&self, args: &[f32]) -> Result<FunctionValue>;
}

impl<Inner: InnerFunction> Function for Inner {
    fn call(&self, args: &[f32]) -> Result<FunctionValue> {
        self.do_call(args)
    }
}

impl Function for Box<dyn Function> {
    fn call(&self, args: &[f32]) -> Result<FunctionValue> {
        self.as_ref().call(args)
    }
}

/// Combine functions to create a new function. These functions called with
/// the same arguments as the original function, and returns only one value.
/// The end result gather the results of the component functions into an vec.
pub struct NFunc(Vec<Box<dyn Function>>);

impl NFunc {
    /// If one element in `functions`, returns it directly.
    /// Returns `NFunc` otherwise.
    pub fn new_box(functions: Vec<Box<dyn Function>>) -> Result<Box<dyn Function>> {
        if functions.len() == 1 {
            Ok(functions
                .into_iter()
                .next()
                .whatever_context("Expected at least one function")?)
        } else {
            Ok(Box::new(Self::new(functions)?))
        }
    }

    /// Returns error if any of the functions has more than one return value.
    pub fn new(functions: Vec<Box<dyn Function>>) -> Result<Self> {
        if functions.is_empty() {
            whatever!("at least one function is required")
        }

        Ok(Self(functions))
    }
}

impl Function for NFunc {
    fn call(&self, args: &[f32]) -> Result<FunctionValue> {
        let mut r = FunctionValue::new();
        for f in &self.0 {
            r.extend_from_slice(&f.call(args)?);
        }
        Ok(r)
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
#[root_pdf_object]
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

pub struct PostScriptFunction {
    signature: Type04Signature,
    f: PdfFunc,
}

impl PostScriptFunction {
    pub fn new(signature: Type04Signature, script: Box<[u8]>) -> Self {
        Self {
            f: PdfFunc::new(script, signature.n_returns()),
            signature,
        }
    }
}

impl InnerFunction for PostScriptFunction {
    type Signature = Type04Signature;

    fn signature(&self) -> &Self::Signature {
        &self.signature
    }

    #[doc = " Called by `self.call()`, args and return value are clipped by signature."]
    fn inner_call(&self, args: FunctionValue) -> Result<FunctionValue> {
        let args = args.into_iter().collect::<Vec<_>>();
        let r = self.f.exec(&args).whatever_context("exec function")?;
        Ok(r.into_iter().collect())
    }
}

impl FunctionDict<'_, '_> {
    fn type23_signature(&self) -> Result<Type23Signature> {
        Ok(Type23Signature {
            domain: self.domain()?,
            range: self.range()?,
        })
    }

    fn type04_signature(&self) -> Result<Type04Signature> {
        Ok(Type04Signature {
            domain: self.domain()?,
            range: self.range()?.whatever_context("range should exist")?,
        })
    }

    pub fn post_script_func(&self) -> Result<PostScriptFunction> {
        ensure_whatever!(
            self.function_type()? == Type::PostScriptCalculator,
            "Type 4 expected"
        );
        let signature = self.type04_signature()?;
        let resolver = self.d.resolver();
        let stream = resolver
            .resolve(self.id)
            .whatever_context("resolve")?
            .stream()
            .whatever_context("get as stream")?;
        let script = stream.decode(resolver).whatever_context("decode stream")?;
        Ok(PostScriptFunction::new(
            signature,
            script.into_owned().into_boxed_slice(),
        ))
    }

    /// Create boxed Function for this Function dict.
    pub fn func(&self) -> Result<Box<dyn Function>> {
        match self.function_type()? {
            Type::Sampled => Ok(Box::new(self.sampled()?.func()?)),
            Type::ExponentialInterpolation => {
                Ok(Box::new(self.exponential_interpolation()?.func()?))
            }
            Type::Stitching => Ok(Box::new(self.stitch()?.func()?)),
            Type::PostScriptCalculator => Ok(Box::new(self.post_script_func()?)),
        }
    }
}

pub trait Signature {
    fn clip_args(&self, args: &[f32]) -> TinyVec<[f32; 4]>;
    fn clip_returns(&self, returns: FunctionValue) -> FunctionValue;
}

/// Function signature for Type 2 and 3, clip input args and returns.
#[derive(Debug, PartialEq, Clone)]
pub struct Type23Signature {
    domain: Domains,
    range: Option<Domains>,
}

impl Signature for Type23Signature {
    fn clip_returns(&self, returns: FunctionValue) -> FunctionValue {
        let Some(range) = self.range.as_ref() else {
            return returns;
        };
        debug_assert_eq!(returns.len(), range.n());

        returns
            .iter()
            .zip(range.0.iter())
            .map(|(&ret, domain)| domain.clamp(ret))
            .collect()
    }

    fn clip_args(&self, args: &[f32]) -> TinyVec<[f32; 4]> {
        debug_assert_eq!(args.len(), self.n_args());

        args.iter()
            .zip(self.domain.0.iter())
            .map(|(&arg, domain)| domain.clamp(arg))
            .collect()
    }
}

impl Type23Signature {
    pub fn new(domain: Domains, range: Option<Domains>) -> Self {
        Self { domain, range }
    }

    pub fn n_args(&self) -> usize {
        self.domain.n()
    }

    pub fn n_returns(&self) -> Option<usize> {
        self.range.as_ref().map(Domains::n)
    }
}

/// Function signature for Type 0 and 4, which range is required
#[derive(Debug, PartialEq, Clone)]
pub struct Type04Signature {
    domain: Domains,
    range: Domains,
}

impl Signature for Type04Signature {
    fn clip_returns(&self, returns: FunctionValue) -> FunctionValue {
        debug_assert_eq!(returns.len(), self.range.n());

        returns
            .iter()
            .zip(self.range.0.iter())
            .map(|(&ret, domain)| domain.clamp(ret))
            .collect()
    }

    fn clip_args(&self, args: &[f32]) -> TinyVec<[f32; 4]> {
        debug_assert_eq!(args.len(), self.n_args());

        args.iter()
            .zip(self.domain.0.iter())
            .map(|(&arg, domain)| domain.clamp(arg))
            .collect()
    }
}

impl Type04Signature {
    pub fn new(domain: Domains, range: Domains) -> Self {
        Self { domain, range }
    }

    pub fn n_args(&self) -> usize {
        self.domain.n()
    }

    pub fn n_returns(&self) -> usize {
        self.range.n()
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
#[root_pdf_object]
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

/// struct to implement Function trait for SampledFunctionDict,
/// because sampled function need to load sample data from stream.
#[derive(Debug, PartialEq, Clone)]
pub struct SampledFunction {
    signature: Type04Signature,
    encode: Domains,
    decode: Domains,
    size: Vec<u32>,
    samples: Vec<u8>,
    bits_per_sample: u8,
}

impl SampledFunction {
    pub fn samples(&self) -> usize {
        // NOTE: assume bits_per_sample is 8
        self.samples.len() / self.signature.n_returns()
    }
}

impl InnerFunction for SampledFunction {
    type Signature = Type04Signature;

    fn inner_call(&self, args: TinyVec<[f32; 4]>) -> Result<FunctionValue> {
        let mut idx = 0;
        for (arg, (domain, (encode, size))) in args
            .iter()
            .zip(
                self.signature
                    .domain
                    .iter()
                    .zip(self.encode.iter().zip(self.size.iter())),
            )
            .rev()
        {
            let arg = (arg - domain.start) / (domain.end - domain.start);
            let arg = arg.mul_add(encode.end - encode.start, encode.start);
            idx = size * idx
                + arg
                    .round()
                    .to_u32()
                    .whatever_context("convert to u32")?
                    .clamp(0, *size - 1);
        }
        let idx = idx as usize;

        let n_ret = self.signature.n_returns();
        let sample_size = self.bits_per_sample as usize / 8;
        let mut r = tiny_vec![];
        let decode = &self.decode.0[0];
        for i in 0..n_ret {
            let start_p = (idx * n_ret + i) * sample_size;
            let mut sample = 0u32;
            for i in 0..sample_size {
                sample <<= 8;
                sample |= self.samples[start_p + i] as u32;
            }
            let sample = sample as f32 / (2.0_f32.powi(self.bits_per_sample as i32) - 1.0);
            let sample = sample.mul_add(decode.end - decode.start, decode.start);
            r.push(sample);
        }
        Ok(r)
    }

    fn signature(&self) -> &Self::Signature {
        &self.signature
    }
}

impl SampledFunctionDict<'_, '_> {
    /// Return SampledFunction instance which implements Function trait.
    pub fn func(&self) -> Result<SampledFunction> {
        let f = self.function_dict()?;
        let bits_per_sample = self.bits_per_sample()?;
        ensure_whatever!(bits_per_sample >= 8, "todo: support bits_per_sample < 8");
        ensure_whatever!(
            InterpolationOrder::Linear == self.order()?,
            "todo: support cubic interpolation"
        );

        let size = self.size()?;
        let resolver = self.d.resolver();
        let stream = resolver
            .resolve(self.id)
            .whatever_context("resolve object")?
            .stream()
            .whatever_context("get as stream")?;
        let sample_data = stream.decode(resolver).whatever_context("decode stream")?;
        let signature = f.type04_signature()?;
        ensure_whatever!(
            sample_data.len() >= size[0] as usize * signature.n_returns(),
            "Sample data length is insufficient"
        );
        Ok(SampledFunction {
            signature,
            encode: self.encode()?.unwrap_or_else(|| {
                Domains(
                    size.iter()
                        .map(|v| Domain::new(0.0, (*v - 1) as f32))
                        .collect(),
                )
            }),
            decode: self.decode()?.map_or_else(
                || {
                    f.range()
                        .whatever_context("get range")?
                        .whatever_context("range should exist in sampled function")
                },
                Ok,
            )?,
            size: self.size()?,
            samples: sample_data.into_owned(),
            bits_per_sample: bits_per_sample.try_into().unwrap_or(u8::MAX),
        })
    }
}

#[pdf_object(2i32)]
#[root_pdf_object]
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
    signature: Type23Signature,
}

impl InnerFunction for ExponentialInterpolationFunction {
    type Signature = Type23Signature;

    fn inner_call(&self, args: TinyVec<[f32; 4]>) -> Result<FunctionValue> {
        let x = args[0];
        let r = (0..self.c0.len())
            .map(|i| x.powf(self.n).mul_add(self.c1[i] - self.c0[i], self.c0[i]))
            .collect();
        Ok(r)
    }

    fn signature(&self) -> &Type23Signature {
        &self.signature
    }
}

impl ExponentialInterpolationFunctionDict<'_, '_> {
    pub fn func(&self) -> Result<ExponentialInterpolationFunction> {
        let f = self.function_dict()?;
        Ok(ExponentialInterpolationFunction {
            c0: self.c0()?,
            c1: self.c1()?,
            n: self.n()?,
            signature: f.type23_signature()?,
        })
    }
}

#[pdf_object(3i32)]
#[root_pdf_object]
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

impl StitchingFunctionDict<'_, '_> {
    pub fn func(&self) -> Result<StitchingFunction> {
        let functions = self
            .functions()?
            .into_iter()
            .map(|f| f.func())
            .collect::<Result<_>>()?;
        let bounds = self.bounds()?;
        let encode = self.encode()?;
        let f = self.function_dict()?;
        let signature = Type23Signature {
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
    signature: Type23Signature,
}

impl StitchingFunction {
    fn find_function(bounds: &[f32], x: f32) -> usize {
        bounds
            .iter()
            .position(|&bound| x < bound)
            .unwrap_or(bounds.len())
    }

    fn sub_domain(domain: Domain, bounds: &[f32], idx: usize) -> Domain {
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

    fn interpolation(from: Domain, to: Domain, t: f32) -> f32 {
        let a_len = from.end - from.start;
        let b_len = to.end - to.start;
        let t = (t - from.start) / a_len;
        t.mul_add(b_len, to.start) // t * b_len + b.start
    }

    fn domains(&self) -> &Domains {
        &self.signature.domain
    }
}

impl InnerFunction for StitchingFunction {
    type Signature = Type23Signature;

    fn inner_call(&self, args: TinyVec<[f32; 4]>) -> Result<FunctionValue> {
        ensure_whatever!(args.len() == 1, "expected one argument");

        let x = args[0];
        let function_idx = Self::find_function(&self.bounds, x);
        let mut sub_domain = Self::sub_domain(self.domains().0[0], &self.bounds, function_idx);
        if sub_domain.is_zero() {
            // possibly incorrect bounds, bounds[0] should > domain[0].start
            // bounds[last] should < domain[0].end, but some buggie file
            // breaks, cause a zero sub_domain
            sub_domain = self.domains().0[0];
        }
        let x1 = Self::interpolation(sub_domain, self.encode.0[function_idx], x);

        let f = &self.functions[function_idx];
        let r = f.call(&[x1])?;
        Ok(r)
    }

    fn signature(&self) -> &Type23Signature {
        &self.signature
    }
}

#[cfg(test)]
mod tests;
