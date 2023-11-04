use std::rc::Rc;

use anyhow::{anyhow, bail, Result as AnyResult};
use euclid::default::{Point3D, Transform3D};
use nipdf_macro::pdf_object;
use smallvec::SmallVec;

use crate::{
    file::{ObjectResolver, ResourceDict},
    function::{Function, FunctionDict, NFunc},
    graphics::ICCStreamDict,
    object::{Object, ObjectValueError},
};

use super::ColorSpaceArgs;

/// Color component composes a color.
/// Two kinds of color component: float or integer.
/// For float color component must in range [0, 1].
pub trait ColorComp: Copy + std::fmt::Debug {
    fn min_color() -> Self;
    /// Max value of color component, for float color component must be 1.0
    fn max_color() -> Self;
}

pub trait ColorCompConvertTo<T: ColorComp> {
    fn into_color_comp(self) -> T;
}

impl ColorCompConvertTo<u8> for u8 {
    fn into_color_comp(self) -> u8 {
        self
    }
}

impl ColorCompConvertTo<f32> for u8 {
    fn into_color_comp(self) -> f32 {
        self as f32 / 255.0
    }
}

impl ColorCompConvertTo<u8> for f32 {
    fn into_color_comp(self) -> u8 {
        // according to pdf file specification, float color component should be
        // rounded to nearest integer, See page 157 of PDF 32000-1:2008
        // If the value is a real number, it shall be rounded to the nearest integer;
        (self * 255.0).round().clamp(0., 255.) as u8
    }
}

impl ColorCompConvertTo<f32> for f32 {
    fn into_color_comp(self) -> f32 {
        self
    }
}

impl ColorComp for u8 {
    fn min_color() -> Self {
        0
    }

    fn max_color() -> Self {
        255
    }
}

impl ColorComp for f32 {
    fn min_color() -> Self {
        0.0
    }

    fn max_color() -> Self {
        1.0
    }
}

fn convert_color_to<F, T, const N: usize>(from: &[F]) -> [T; N]
where
    F: ColorComp + ColorCompConvertTo<T>,
    T: ColorComp,
{
    std::array::from_fn(|i| from[i].into_color_comp())
}

/// Convert color to rgba color space, convert result to f32 or u8 by T generic type.
pub fn color_to_rgba<F, T, CS>(cs: &CS, color: &[F]) -> [T; 4]
where
    F: ColorComp,
    T: ColorComp,
    CS: ColorSpaceTrait<F>,
    F: ColorCompConvertTo<T>,
{
    convert_color_to(&cs.to_rgba(color))
}

#[derive(Debug, Clone, PartialEq)]
pub enum ColorSpace<T = f32> {
    DeviceGray,
    DeviceRGB,
    DeviceCMYK,
    Pattern,
    Indexed(Box<IndexedColorSpace<T>>),
    Separation(Box<SeparationColorSpace<T>>),
    CalRGB(Box<CalRGBColorSpace>),
    /// Without this, complier complains T is not referenced in any of enum branches
    _Phantom(T),
}

impl<T> ColorSpace<T>
where
    T: ColorComp + ColorCompConvertTo<f32> + 'static,
    T: ColorCompConvertTo<u8>,
    f32: ColorCompConvertTo<T>,
    u8: ColorCompConvertTo<T>,
{
    /// Create from color space args.
    /// Panic if need resolve resource but resources is None.
    pub fn from_args<'a>(
        args: &ColorSpaceArgs,
        resolver: &ObjectResolver<'a>,
        resources: Option<&ResourceDict<'a, '_>>,
    ) -> AnyResult<Self> {
        match args {
            ColorSpaceArgs::Ref(id) => {
                let obj = resolver.resolve(*id)?;
                let args = ColorSpaceArgs::try_from(obj)?;
                Self::from_args(&args, resolver, resources)
            }
            ColorSpaceArgs::Name(name) => {
                let name = name.as_ref();
                match name {
                    "DeviceGray" => Ok(Self::DeviceGray),
                    "DeviceRGB" => Ok(Self::DeviceRGB),
                    "DeviceCMYK" => Ok(Self::DeviceCMYK),
                    "Pattern" => Ok(Self::Pattern),
                    _ => {
                        let color_spaces = resources.unwrap().color_space()?;
                        let args = color_spaces.get(name).ok_or_else(|| {
                            anyhow!("ColorSpace::from_args() color space not found")
                        })?;
                        Self::from_args(args, resolver, resources)
                    }
                }
            }
            ColorSpaceArgs::Array(arr) => match arr[0].as_name()? {
                "ICCBased" => {
                    debug_assert_eq!(2, arr.len());
                    let id = arr[1].as_ref()?;
                    let d: ICCStreamDict = resolver.resolve_pdf_object(id.id().id())?;
                    match d.alternate()?.as_ref() {
                        Some(args) => Self::from_args(args, resolver, resources),
                        None => match d.n()? {
                            1 => Ok(Self::DeviceGray),
                            3 => Ok(Self::DeviceRGB),
                            4 => Ok(Self::DeviceCMYK),
                            _ => unreachable!("ICC color space n value should be 1, 3 or 4"),
                        },
                    }
                }
                "Separation" => {
                    debug_assert_eq!(4, arr.len());
                    let alternate = ColorSpaceArgs::try_from(&arr[2])?;
                    let functions: Vec<FunctionDict> =
                        resolver.resolve_one_or_more_pdf_object(arr[3].as_ref()?.id().id())?;
                    let functions: Result<Vec<_>, _> =
                        functions.into_iter().map(|f| f.func()).collect();
                    let function = NFunc::new_box(functions?)?;
                    let base = Self::from_args(&alternate, resolver, resources)?;
                    Ok(Self::Separation(Box::new(SeparationColorSpace {
                        base,
                        f: Rc::new(function),
                    })))
                }
                "Indexed" => {
                    debug_assert_eq!(4, arr.len());
                    let base = ColorSpaceArgs::try_from(&arr[1])?;
                    let base: ColorSpace<T> = Self::from_args(&base, resolver, resources)?;
                    let hival = arr[2].as_int()?;
                    let data = resolve_index_data(&arr[3], resolver)?;
                    assert!(data.len() >= (hival + 1) as usize * base.components());
                    Ok(Self::Indexed(Box::new(IndexedColorSpace { base, data })))
                }
                s => todo!("ColorSpace::from_args() {} color space", s),
            },
        }
    }
}

/// Resolve data for indexed color space, it may exist in stream or HexString or LiteralString
fn resolve_index_data(o: &Object, resolver: &ObjectResolver) -> AnyResult<Vec<u8>> {
    Ok(match o {
        Object::HexString(s) => s.decoded()?,
        Object::LiteralString(s) => s.decode_to_bytes()?,
        Object::Reference(id) => {
            let o = resolver.resolve(id.id().id())?;
            match o {
                Object::HexString(s) => s.decoded()?,
                Object::LiteralString(s) => s.decode_to_bytes()?,
                Object::Stream(s) => s.decode(resolver)?.into_owned(),
                _ => bail!("Unexpected object type when resolve indexed color space data"),
            }
        }
        _ => bail!("Unexpected object type when resolve indexed color space data"),
    })
}

impl<T> ColorSpaceTrait<T> for ColorSpace<T>
where
    T: ColorComp + ColorCompConvertTo<f32> + 'static,
    T: ColorCompConvertTo<u8>,
    f32: ColorCompConvertTo<T>,
    u8: ColorCompConvertTo<T>,
{
    fn to_rgba(&self, color: &[T]) -> [T; 4] {
        match self {
            Self::DeviceGray => DeviceGray.to_rgba(color),
            Self::DeviceRGB => DeviceRGB.to_rgba(color),
            Self::DeviceCMYK => DeviceCMYK.to_rgba(color),
            Self::Pattern => PatternColorSpace.to_rgba(color),
            Self::Indexed(indexed) => indexed.to_rgba(color),
            Self::Separation(sep) => sep.as_ref().to_rgba(color),
            Self::CalRGB(cal_rgb) => {
                let p: [f32; 4] = convert_color_to(color);
                cal_rgb.to_rgba(&p);
                convert_color_to(&p)
            }
            Self::_Phantom(_) => unreachable!(),
        }
    }

    fn components(&self) -> usize {
        match self {
            Self::DeviceGray => ColorSpaceTrait::<T>::components(&DeviceGray),
            Self::DeviceRGB => ColorSpaceTrait::<T>::components(&DeviceRGB),
            Self::DeviceCMYK => ColorSpaceTrait::<T>::components(&DeviceCMYK),
            Self::Pattern => ColorSpaceTrait::<T>::components(&PatternColorSpace),
            Self::Indexed(indexed) => indexed.components(),
            Self::Separation(sep) => sep.as_ref().components(),
            Self::CalRGB(cal_rgb) => cal_rgb.components(),
            Self::_Phantom(_) => unreachable!(),
        }
    }
}

pub trait ColorSpaceTrait<T> {
    /// Convert color from current space to RGBA.
    /// `color` len should at least be `components()`
    /// Use `color_to_rgba()` function, if target color space is not T.
    fn to_rgba(&self, color: &[T]) -> [T; 4];

    /// Number of color components in this color space.
    fn components(&self) -> usize;

    /// Convert color from current space to RGBA tiny_skia color
    /// `color` len should at least be `components()`
    fn to_skia_color(&self, color: &[T]) -> tiny_skia::Color
    where
        T: ColorComp + ColorCompConvertTo<f32>,
    {
        let rgba = self.to_rgba(color);
        let rgba: [_; 4] = convert_color_to(&rgba);
        tiny_skia::Color::from_rgba(rgba[0], rgba[1], rgba[2], rgba[3]).unwrap()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DeviceGray;

impl<T: ColorComp> ColorSpaceTrait<T> for DeviceGray {
    fn to_rgba(&self, color: &[T]) -> [T; 4] {
        [color[0], color[0], color[0], T::max_color()]
    }

    fn components(&self) -> usize {
        1
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DeviceRGB;

impl<T: ColorComp> ColorSpaceTrait<T> for DeviceRGB {
    fn to_rgba(&self, color: &[T]) -> [T; 4] {
        [color[0], color[1], color[2], T::max_color()]
    }

    fn components(&self) -> usize {
        3
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DeviceCMYK;

impl<T> ColorSpaceTrait<T> for DeviceCMYK
where
    T: ColorComp + ColorCompConvertTo<f32>,
    f32: ColorCompConvertTo<T>,
{
    fn to_rgba(&self, color: &[T]) -> [T; 4] {
        let c = color[0].into_color_comp();
        let m = color[1].into_color_comp();
        let y = color[2].into_color_comp();
        let k = color[3].into_color_comp();
        let c1 = 1.0 - c;
        let m1 = 1.0 - m;
        let y1 = 1.0 - y;
        let k1 = 1.0 - k;

        let x = c1 * m1 * y1 * k1; // 0 0 0 0
        let (mut r, mut g, mut b) = (x, x, x);

        let x = c1 * m1 * y1 * k; // 0 0 0 1
        r += 0.1373 * x;
        g += 0.1216 * x;
        b += 0.1255 * x;

        let x = c1 * m1 * y * k1; // 0 0 1 0
        r += x;
        g += 0.9490 * x;

        let x = c1 * m1 * y * k; // 0 0 1 1
        r += 0.1098 * x;
        g += 0.1020 * x;
        let x = c1 * m * y1 * k1; // 0 1 0 0
        r += 0.9255 * x;
        b += 0.5490 * x;
        let x = c1 * m * y1 * k; // 0 1 0 1
        r += 0.1412 * x;
        let x = c1 * m * y * k1; // 0 1 1 0
        r += 0.9294 * x;
        g += 0.1098 * x;
        b += 0.1412 * x;
        let x = c1 * m * y * k; // 0 1 1 1
        r += 0.1333 * x;
        let x = c * m1 * y1 * k1; // 1 0 0 0
        g += 0.6784 * x;
        b += 0.9373 * x;
        let x = c * m1 * y1 * k; // 1 0 0 1
        g += 0.0588 * x;
        b += 0.1412 * x;
        let x = c * m1 * y * k1; // 1 0 1 0
        g += 0.6510 * x;
        b += 0.3137 * x;
        let x = c * m1 * y * k; // 1 0 1 1
        g += 0.0745 * x;
        let x = c * m * y1 * k1; // 1 1 0 0
        r += 0.1804 * x;
        g += 0.1922 * x;
        b += 0.5725 * x;
        let x = c * m * y1 * k; // 1 1 0 1
        b += 0.0078 * x;
        let x = c * m * y * k1; // 1 1 1 0
        r += 0.2118 * x;
        g += 0.2119 * x;
        b += 0.2235 * x;

        [
            r.into_color_comp(),
            g.into_color_comp(),
            b.into_color_comp(),
            T::max_color(),
        ]
    }

    fn components(&self) -> usize {
        4
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PatternColorSpace;

impl<T> ColorSpaceTrait<T> for PatternColorSpace {
    fn to_rgba(&self, _color: &[T]) -> [T; 4] {
        unreachable!("PatternColorSpace.to_rgba() should not be called")
    }

    fn components(&self) -> usize {
        0
    }
}

/// Indexed Color Space, access color by index, resolve the real color
/// using base color space. The index is 1 byte.
/// Base color stored in data, each color component is a u8. Max index
/// is data.len() / base.components().
#[derive(Debug, Clone, PartialEq)]
pub struct IndexedColorSpace<T> {
    pub base: ColorSpace<T>,
    pub data: Vec<u8>,
}

impl<T> IndexedColorSpace<T>
where
    T: ColorComp + ColorCompConvertTo<f32> + 'static,
    T: ColorCompConvertTo<u8>,
    f32: ColorCompConvertTo<T>,
    u8: ColorCompConvertTo<T>,
{
    /// Counts of colors in this color space.
    /// Max index is this value - 1.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.data.len() / self.base.components()
    }
}

impl<T> ColorSpaceTrait<T> for IndexedColorSpace<T>
where
    T: ColorComp + ColorCompConvertTo<f32> + 'static,
    T: ColorCompConvertTo<u8>,
    f32: ColorCompConvertTo<T>,
    u8: ColorCompConvertTo<T>,
{
    fn to_rgba(&self, color: &[T]) -> [T; 4] {
        let index = ColorCompConvertTo::<u8>::into_color_comp(color[0]) as usize;
        let n = self.base.components();
        let u8_color = &self.data[index * n..(index + 1) * n];
        let c: [T; 4] = std::array::from_fn(|i| {
            if i == 3 {
                T::max_color()
            } else {
                u8_color[i].into_color_comp()
            }
        });

        self.base.to_rgba(&c)
    }

    fn components(&self) -> usize {
        1
    }
}

#[derive(Clone)]
pub struct SeparationColorSpace<T> {
    base: ColorSpace<T>,

    // use Rc, because Box not impl clone trait
    f: Rc<dyn Function>,
}

impl<T: core::fmt::Debug> core::fmt::Debug for SeparationColorSpace<T> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        let mut builder = formatter.debug_struct("SeparationColorSpace");
        builder.field("base", &self.base);
        builder.finish()
    }
}

impl<T: PartialEq> core::cmp::PartialEq for SeparationColorSpace<T> {
    fn eq(&self, other: &Self) -> bool {
        core::cmp::PartialEq::eq(&self.base, &other.base)
    }
}

impl<T> ColorSpaceTrait<T> for SeparationColorSpace<T>
where
    T: ColorComp + ColorCompConvertTo<f32> + 'static,
    T: ColorCompConvertTo<u8>,
    f32: ColorCompConvertTo<T>,
    u8: ColorCompConvertTo<T>,
{
    fn to_rgba(&self, color: &[T]) -> [T; 4] {
        let c = self.f.call(&[color[0].into_color_comp()]).unwrap();
        let c: SmallVec<[T; 4]> = c.iter().map(|v| v.into_color_comp()).collect();
        self.base.to_rgba(c.as_slice())
    }

    fn components(&self) -> usize {
        1
    }
}

#[pdf_object(())]
trait CalRGBDictTrait {
    #[try_from]
    fn gamma(&self) -> [f32; 3];

    #[try_from]
    #[default_fn(default_matrix)]
    fn matrix(&self) -> [f32; 9];

    #[try_from]
    #[or_default]
    fn black_point(&self) -> [f32; 3];

    #[try_from]
    #[default_fn(default_white_point)]
    fn white_point(&self) -> [f32; 3];
}

fn default_matrix() -> [f32; 9] {
    [
        1.0, 0.0, 0.0, // line 1
        0.0, 1.0, 0.0, // line 2
        0.0, 0.0, 1.0, // line 3
    ]
}

fn default_white_point() -> [f32; 3] {
    [1.0, 1.0, 1.0]
}

impl<'a> TryFrom<&Object<'a>> for Point3D<f32> {
    type Error = ObjectValueError;

    fn try_from(obj: &Object<'a>) -> Result<Self, Self::Error> {
        let arr = obj.as_arr()?;
        if arr.len() != 3 {
            return Err(ObjectValueError::UnexpectedType);
        }
        Ok(Point3D::new(
            arr[0].as_number()?,
            arr[1].as_number()?,
            arr[2].as_number()?,
        ))
    }
}

impl<'a> TryFrom<&Object<'a>> for Transform3D<f32> {
    type Error = ObjectValueError;

    fn try_from(obj: &Object<'a>) -> Result<Self, Self::Error> {
        let arr = obj.as_arr()?;
        if arr.len() != 9 {
            return Err(ObjectValueError::UnexpectedType);
        }
        let mut m = [0.0; 9];
        for (i, v) in arr.iter().enumerate() {
            m[i] = v.as_number()?;
        }

        // No 3x3 transform in euclid crate
        Ok(Transform3D::new(
            m[0], m[1], m[2], 0.0, // line 1
            m[3], m[4], m[5], 0.0, // line 2
            m[6], m[7], m[8], 0.0, // line 3
            0.0, 0.0, 0.0, 1.0, // line 4
        ))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CalRGBColorSpace {
    gamma: [f32; 3],
    matrix: [f32; 9],
    black_point: [f32; 3],
    white_point: [f32; 3],
}

impl ColorSpaceTrait<f32> for CalRGBColorSpace {
    fn to_rgba(&self, color: &[f32]) -> [f32; 4] {
        let ag = color[0].powf(self.gamma[0]);
        let bg = color[1].powf(self.gamma[1]);
        let cg = color[2].powf(self.gamma[2]);

        let x = self.matrix[0] * ag + self.matrix[1] * bg + self.matrix[2] * cg;
        let y = self.matrix[3] * ag + self.matrix[4] * bg + self.matrix[5] * cg;
        let z = self.matrix[6] * ag + self.matrix[7] * bg + self.matrix[8] * cg;
        let x = x.clamp(0.0, 1.0);
        let y = y.clamp(0.0, 1.0);
        let z = z.clamp(0.0, 1.0);

        [x, y, z, 1.0]
    }

    fn components(&self) -> usize {
        3
    }
}

#[cfg(test)]
mod tests;
