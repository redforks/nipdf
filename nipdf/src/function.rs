use crate::{
    ObjectValueError, Result,
    file::ObjectResolver,
    object::{
        FromSchemaContainer, Object, ObjectWithResolver, PdfObject as _, RootPdfObject as _,
        RuntimeObjectId,
    },
};
use educe::Educe;
#[cfg(test)]
use mockall::automock;
use nipdf_macro::{TryFromIntObject, pdf_object};
use num_traits::ToPrimitive;
use prescript::PdfFunc;
use snafu::{OptionExt as _, ResultExt, ensure_whatever, whatever};
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

impl TryFrom<ObjectWithResolver<'_, '_>> for Domain<f32> {
    type Error = ObjectValueError;

    fn try_from(obj: ObjectWithResolver<'_, '_>) -> Result<Self, Self::Error> {
        let arr = obj.into_schema_array()?;
        ensure_whatever!(
            arr.len() == 2,
            "expected array with 2 elements, but got {}",
            arr.len()
        );
        Ok(Self::new(
            arr.required_object(0)?.number()?,
            arr.required_object(1)?.number()?,
        ))
    }
}

impl TryFrom<ObjectWithResolver<'_, '_>> for Domain<u32> {
    type Error = ObjectValueError;

    fn try_from(obj: ObjectWithResolver<'_, '_>) -> Result<Self, Self::Error> {
        let arr = obj.into_schema_array()?;
        ensure_whatever!(
            arr.len() == 2,
            "expected array with 2 elements, but got {}",
            arr.len()
        );
        Ok(Self::new(
            arr.required_object(0)?.int()? as u32,
            arr.required_object(1)?.int()? as u32,
        ))
    }
}

#[derive(Debug, PartialEq, Clone, Educe)]
#[educe(Deref)]
pub struct Domains<T = f32>(pub Vec<Domain<T>>);

impl TryFrom<&Object> for Domains<f32> {
    type Error = ObjectValueError;

    fn try_from(obj: &Object) -> Result<Self, Self::Error> {
        let arr = obj.as_arr()?;
        ensure_whatever!(arr.len() % 2 == 0, "even number of elements expected");
        let mut domains = Vec::with_capacity(arr.len() / 2);
        arr.chunks_exact(2)
            .map(|chunk| {
                Ok::<_, ObjectValueError>(Domain::new(chunk[0].number()?, chunk[1].number()?))
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .for_each(|domain| domains.push(domain));
        Ok(Self(domains))
    }
}

impl TryFrom<ObjectWithResolver<'_, '_>> for Domains<f32> {
    type Error = ObjectValueError;

    fn try_from(obj: ObjectWithResolver<'_, '_>) -> Result<Self, Self::Error> {
        let arr = obj.into_schema_array()?;
        ensure_whatever!(arr.len() % 2 == 0, "even number of elements expected");
        let mut domains = Vec::with_capacity(arr.len() / 2);
        for i in (0..arr.len()).step_by(2) {
            domains.push(Domain::new(
                arr.required_object(i)?.number()?,
                arr.required_object(i + 1)?.number()?,
            ));
        }
        Ok(Self(domains))
    }
}

impl TryFrom<&Object> for Domains<u32> {
    type Error = ObjectValueError;

    fn try_from(obj: &Object) -> Result<Self, Self::Error> {
        let arr = obj.as_arr()?;
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

    /// Return the domain stops of the function. Helps to build shading stops.
    fn stops(&self) -> impl Iterator<Item = f32> + 'static;
}

#[cfg_attr(test, automock)]
pub trait Function {
    fn call(&self, args: &[f32]) -> Result<FunctionValue>;
    fn stops(&self) -> Box<dyn Iterator<Item = f32>>;
    fn n_out(&self) -> u8;
}

impl<Inner: InnerFunction> Function for Inner {
    fn call(&self, args: &[f32]) -> Result<FunctionValue> {
        self.do_call(args)
    }

    fn stops(&self) -> Box<dyn Iterator<Item = f32>> {
        Box::new(self.stops())
    }

    fn n_out(&self) -> u8 {
        self.signature().n_out()
    }
}

impl Function for Box<dyn Function> {
    fn call(&self, args: &[f32]) -> Result<FunctionValue> {
        self.as_ref().call(args)
    }

    fn stops(&self) -> Box<dyn Iterator<Item = f32>> {
        self.as_ref().stops()
    }

    fn n_out(&self) -> u8 {
        self.as_ref().n_out()
    }
}

impl<'a, 'b> FromSchemaContainer<'a, 'b> for Box<dyn Function> {
    fn create(o: &'b Object, r: &'b ObjectResolver<'a>) -> Result<Self, ObjectValueError> {
        let id: Option<RuntimeObjectId> = o.reference().ok().map(Into::into);
        let dict = r.resolve_reference(o)?.as_dict()?;

        // Get function type from dictionary
        let function_type = dict
            .get("FunctionType")
            .ok_or(ObjectValueError::DictKeyNotFound)?;
        let function_type = Type::try_from(function_type)?;

        // Create function based on type
        match function_type {
            Type::Sampled => {
                let dict = SampledFunctionDict::new(
                    id.whatever_context::<_, ObjectValueError>(
                        "Sampled function should be root object",
                    )?,
                    dict,
                    r,
                )?;
                Ok(Box::new(
                    dict.func()
                        .whatever_context::<_, ObjectValueError>("Parse Sampled function")?,
                ))
            }

            Type::ExponentialInterpolation => {
                let dict = ExponentialInterpolationFunctionDict::new(dict, r)?;
                Ok(Box::new(
                    dict.func().whatever_context::<_, ObjectValueError>(
                        "Parse Exponential Interpolation function",
                    )?,
                ))
            }

            Type::Stitching => {
                let dict = StitchingFunctionDict::new(dict, r)?;
                Ok(Box::new(
                    dict.func()
                        .whatever_context::<_, ObjectValueError>("Parse Stitching function")?,
                ))
            }

            Type::PostScriptCalculator => {
                // Get required fields
                let domain = dict
                    .get("Domain")
                    .ok_or(ObjectValueError::DictKeyNotFound)
                    .and_then(Domains::try_from)?;

                let range = dict
                    .get("Range")
                    .ok_or(ObjectValueError::DictKeyNotFound)
                    .and_then(Domains::try_from)?;

                let signature = Type04Signature::new(domain, range);

                // Get stream data
                let stream = r.resolve_reference(o)?.as_stream()?;
                let script = stream
                    .decode(r)
                    .whatever_context::<_, ObjectValueError>("decode stream")?;

                Ok(Box::new(PostScriptFunction::new(
                    signature,
                    script.into_owned().into_boxed_slice(),
                )))
            }
        }
    }
}

/// Combine functions to create a new function. These functions called with
/// the same arguments as the original function, and returns only one value.
/// The end result gather the results of the component functions into an vec.
pub struct NFunc(Vec<Box<dyn Function>>);

impl NFunc {
    /// If one element in `functions`, returns it directly.
    /// If the first function has >1 return value, returns it directly.
    /// Returns `NFunc` otherwise.
    pub fn new_box(functions: Vec<Box<dyn Function>>) -> Result<Box<dyn Function>> {
        if let Some(first) = functions.first() {
            if functions.len() == 1 || first.n_out() > 1 {
                return functions
                    .into_iter()
                    .next()
                    .whatever_context::<_, ObjectValueError>("Expected at least one function");
            }
        }
        Ok(Box::new(Self::new(functions)?))
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

    fn stops(&self) -> Box<dyn Iterator<Item = f32>> {
        // Collect all stops from all functions
        let mut stops = Vec::new();
        for f in &self.0 {
            stops.extend(f.stops());
        }

        // Sort and deduplicate
        stops.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        stops.dedup_by(|a, b| (*a - *b).abs() < f32::EPSILON);

        Box::new(stops.into_iter())
    }

    fn n_out(&self) -> u8 {
        self.0.first().map_or(0, Function::n_out)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, TryFromIntObject)]
pub enum Type {
    Sampled = 0,
    ExponentialInterpolation = 2,
    Stitching = 3,
    PostScriptCalculator = 4,
}

pub struct PostScriptFunction {
    signature: Type04Signature,
    f: PdfFunc,
}

impl PostScriptFunction {
    pub fn new(signature: Type04Signature, script: Box<[u8]>) -> Self {
        Self {
            f: PdfFunc::new(script, signature.n_out()),
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
        let r = self
            .f
            .exec(&args)
            .whatever_context::<_, ObjectValueError>("exec function")?;
        Ok(r.into_iter().collect())
    }

    fn stops(&self) -> impl Iterator<Item = f32> + 'static {
        Err("TODO: PostScriptFunction::stops()").into_iter()
    }
}

pub trait Signature {
    fn clip_args(&self, args: &[f32]) -> TinyVec<[f32; 4]>;
    fn clip_returns(&self, returns: FunctionValue) -> FunctionValue;
    fn n_out(&self) -> u8;
}

/// Function signature for Type 2 and 3, clip input args and returns.
#[derive(Debug, PartialEq, Clone)]
pub struct Type23Signature {
    n: u8,
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

    fn n_out(&self) -> u8 {
        self.n
    }
}

impl Type23Signature {
    pub fn n_args(&self) -> usize {
        self.domain.n()
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

    fn n_out(&self) -> u8 {
        #[allow(clippy::unwrap_used)] // impossible to have more than u8::MAX outputs
        self.domain.len().try_into().unwrap()
    }
}

impl Type04Signature {
    pub fn new(domain: Domains, range: Domains) -> Self {
        Self { domain, range }
    }

    pub fn n_args(&self) -> usize {
        self.domain.n()
    }

    pub fn n_out(&self) -> usize {
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
    fn size(&self) -> Vec<u32>;
    fn bits_per_sample(&self) -> u32;

    #[try_from]
    #[or_default]
    fn order(&self) -> InterpolationOrder;

    #[try_from]
    fn encode(&self) -> Option<Domains>;

    #[try_from]
    fn decode(&self) -> Option<Domains>;

    #[try_from]
    fn domain(&self) -> Domains;

    #[try_from]
    fn range(&self) -> Option<Domains>;
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
    fn samples(&self) -> usize {
        // NOTE: assume bits_per_sample is 8
        self.samples.len() / self.signature.n_out()
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
                    .whatever_context::<_, ObjectValueError>("convert to u32")?
                    .clamp(0, *size - 1);
        }
        let idx = idx as usize;

        let n_ret = self.signature.n_out();
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

    fn stops(&self) -> impl Iterator<Item = f32> + 'static {
        let domain = &self.signature.domain.0[0];
        let samples = self.samples().min(256);
        let t0 = euclid::default::Length::new(domain.start);
        let t1 = euclid::default::Length::new(domain.end);

        (0..samples).map(move |i| t0.lerp(t1, i as f32 / (samples - 1) as f32).0)
    }
}

impl SampledFunctionDict<'_, '_> {
    fn type04_signature(&self) -> Result<Type04Signature> {
        Ok(Type04Signature {
            domain: self.domain()?,
            range: self
                .range()?
                .whatever_context::<_, ObjectValueError>("range should exist")?,
        })
    }

    /// Return SampledFunction instance which implements Function trait.
    pub fn func(&self) -> Result<SampledFunction> {
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
            .whatever_context::<_, ObjectValueError>("resolve object")?
            .as_stream()
            .whatever_context::<_, ObjectValueError>("get as stream")?;
        let sample_data = stream
            .decode(resolver)
            .whatever_context::<_, ObjectValueError>("decode stream")?;
        let signature = self.type04_signature()?;
        ensure_whatever!(
            sample_data.len() >= size[0] as usize * signature.n_out(),
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
                    self.range()
                        .whatever_context::<_, ObjectValueError>("get range")?
                        .whatever_context::<_, ObjectValueError>(
                            "range should exist in sampled function",
                        )
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
#[type_field("FunctionType")]
pub trait ExponentialInterpolationFunctionDictTrait {
    #[default_fn(f32_zero_arr)]
    fn c0(&self) -> Vec<f32>;

    #[default_fn(f32_one_arr)]
    fn c1(&self) -> Vec<f32>;

    fn n(&self) -> f32;

    #[try_from]
    fn domain(&self) -> Domains;

    #[try_from]
    fn range(&self) -> Option<Domains>;
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

    fn stops(&self) -> impl Iterator<Item = f32> + 'static {
        // For exponential interpolation, we just need start and end points
        // of the domain, similar to how build_stops() handles it
        let domain = &self.signature.domain.0[0]; // Get first domain
        vec![domain.start, domain.end].into_iter()
    }
}

impl ExponentialInterpolationFunctionDict<'_, '_> {
    fn func(&self) -> Result<ExponentialInterpolationFunction> {
        Ok(ExponentialInterpolationFunction {
            c0: self.c0()?,
            c1: self.c1()?,
            n: self.n()?,
            signature: self.type23_signature()?,
        })
    }

    fn type23_signature(&self) -> Result<Type23Signature> {
        Ok(Type23Signature {
            domain: self.domain()?,
            range: self.range()?,
            n: self
                .c0()?
                .len()
                .try_into()
                .whatever_context::<_, ObjectValueError>("cast n-out")?,
        })
    }
}

#[pdf_object(3i32)]
#[type_field("FunctionType")]
pub trait StitchingFunctionDictTrait {
    /// The number of values shall be `k - 1`
    fn bounds(&self) -> Vec<f32>;

    /// The number of values shall be `k`
    #[try_from]
    fn encode(&self) -> Domains;

    #[try_from]
    fn domain(&self) -> Domains;

    #[try_from]
    fn range(&self) -> Option<Domains>;
}

impl StitchingFunctionDict<'_, '_> {
    fn func(&self) -> Result<StitchingFunction> {
        let functions: Vec<Box<dyn Function>> =
            self.d
                .zero_one_or_more("Functions")
                .whatever_context::<_, ObjectValueError>("get stitching Functions")?;
        let bounds = self.bounds()?;
        let encode = self.encode()?;
        let signature = Type23Signature {
            domain: self.domain()?,
            range: self.range()?,
            n: functions.first().map_or(0, Function::n_out),
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

    fn stops(&self) -> impl Iterator<Item = f32> + 'static {
        let domain = self.domains().0[0];
        let mut stops = Vec::with_capacity(self.bounds.len() + 2);
        stops.push(domain.start);
        for t in &self.bounds {
            stops.push(*t);
        }
        stops.push(domain.end);
        stops.into_iter()
    }
}

#[cfg(test)]
mod tests;
