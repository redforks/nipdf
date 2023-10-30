use std::rc::Rc;

use anyhow::{anyhow, Result as AnyResult};
use educe::Educe;
use tinyvec::ArrayVec;

use crate::{
    file::{ObjectResolver, ResourceDict},
    function::{Function, FunctionDict},
    graphics::ICCStreamDict,
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

// pub trait ColorSpaceBoxClone<T> {
//     fn clone_box(&self) -> Box<dyn ColorSpaceTrait<T>>;
// }

// impl<T, O: Clone + ColorSpaceTrait<T> + 'static> ColorSpaceBoxClone<T> for O {
//     fn clone_box(&self) -> Box<dyn ColorSpaceTrait<T>> {
//         Box::new(self.clone())
//     }
// }

/// Convert color to rgba color space, convert result to f32 or u8 by T generic type.
pub fn color_to_rgba<F, T, CS>(cs: &CS, color: &[F]) -> [T; 4]
where
    F: ColorComp,
    T: ColorComp,
    CS: ColorSpaceTrait<F>,
    F: ColorCompConvertTo<T>,
{
    let rgba = cs.to_rgba(color);
    [
        rgba[0].into_color_comp(),
        rgba[1].into_color_comp(),
        rgba[2].into_color_comp(),
        rgba[3].into_color_comp(),
    ]
}

#[derive(Debug, Clone, PartialEq)]
pub enum ColorSpace<T: PartialEq + std::fmt::Debug = f32> {
    DeviceGray,
    DeviceRGB,
    DeviceCMYK,
    Pattern,
    Indexed(Box<IndexedColorSpace<T>>),
    Separation(Box<SeparationColorSpace<T>>),
    Phantom(T),
}

impl<T: PartialEq + std::fmt::Debug> ColorSpace<T> {
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
                    "DeviceGray" => Ok(ColorSpace::DeviceGray),
                    "DeviceRGB" => Ok(ColorSpace::DeviceRGB),
                    "DeviceCMYK" => Ok(ColorSpace::DeviceCMYK),
                    "Pattern" => Ok(ColorSpace::Pattern),
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
                            1 => Ok(ColorSpace::DeviceGray),
                            3 => Ok(ColorSpace::DeviceRGB),
                            4 => Ok(ColorSpace::DeviceCMYK),
                            _ => unreachable!("ICC color space n value should be 1, 3 or 4"),
                        },
                    }
                }
                "Separation" => {
                    debug_assert_eq!(4, arr.len());
                    let alternate = ColorSpaceArgs::try_from(&arr[2])?;
                    let function: FunctionDict =
                        resolver.resolve_pdf_object(arr[3].as_ref()?.id().id())?;
                    let base = Self::from_args(&alternate, resolver, resources)?;
                    Ok(ColorSpace::Separation(Box::new(SeparationColorSpace {
                        base,
                        f: Rc::new(function.func()?),
                    })))
                }
                _ => todo!(),
            },
        }
    }
}

impl<T> ColorSpaceTrait<T> for ColorSpace<T>
where
    T: ColorComp + ColorCompConvertTo<f32> + Default + PartialEq + 'static,
    T: ColorCompConvertTo<u8>,
    f32: ColorCompConvertTo<T>,
    u8: ColorCompConvertTo<T>,
{
    fn to_rgba(&self, color: &[T]) -> [T; 4] {
        match self {
            ColorSpace::DeviceGray => DeviceGray().to_rgba(color),
            ColorSpace::DeviceRGB => DeviceRGB().to_rgba(color),
            ColorSpace::DeviceCMYK => DeviceCMYK().to_rgba(color),
            ColorSpace::Pattern => PatternColorSpace().to_rgba(color),
            ColorSpace::Indexed(indexed) => indexed.to_rgba(color),
            ColorSpace::Separation(sep) => sep.as_ref().to_rgba(color),
            ColorSpace::Phantom(_) => unreachable!(),
        }
    }

    fn components(&self) -> usize {
        match self {
            ColorSpace::DeviceGray => ColorSpaceTrait::<T>::components(&DeviceGray()),
            ColorSpace::DeviceRGB => ColorSpaceTrait::<T>::components(&DeviceRGB()),
            ColorSpace::DeviceCMYK => ColorSpaceTrait::<T>::components(&DeviceCMYK()),
            ColorSpace::Pattern => ColorSpaceTrait::<T>::components(&PatternColorSpace()),
            ColorSpace::Indexed(indexed) => indexed.components(),
            ColorSpace::Separation(sep) => sep.as_ref().components(),
            ColorSpace::Phantom(_) => unreachable!(),
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
        tiny_skia::Color::from_rgba(
            rgba[0].into_color_comp(),
            rgba[1].into_color_comp(),
            rgba[2].into_color_comp(),
            rgba[3].into_color_comp(),
        )
        .unwrap()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DeviceGray();

impl<T: ColorComp> ColorSpaceTrait<T> for DeviceGray {
    fn to_rgba(&self, color: &[T]) -> [T; 4] {
        [color[0], color[0], color[0], T::max_color()]
    }

    fn components(&self) -> usize {
        1
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DeviceRGB();

impl<T: ColorComp> ColorSpaceTrait<T> for DeviceRGB {
    fn to_rgba(&self, color: &[T]) -> [T; 4] {
        [color[0], color[1], color[2], T::max_color()]
    }

    fn components(&self) -> usize {
        3
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DeviceCMYK();

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
pub struct PatternColorSpace();

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
pub struct IndexedColorSpace<T: PartialEq + std::fmt::Debug> {
    pub base: ColorSpace<T>,
    pub data: Vec<u8>,
}

impl<T> IndexedColorSpace<T>
where
    T: ColorComp + ColorCompConvertTo<f32> + Default + PartialEq + 'static,
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
    T: ColorComp + ColorCompConvertTo<f32> + Default + PartialEq + 'static,
    T: ColorCompConvertTo<u8>,
    f32: ColorCompConvertTo<T>,
    u8: ColorCompConvertTo<T>,
{
    fn to_rgba(&self, color: &[T]) -> [T; 4] {
        let index = ColorCompConvertTo::<u8>::into_color_comp(color[0]) as usize;
        let n = self.base.components();
        let u8_color = &self.data[index * n..(index + 1) * n];
        let c: ArrayVec<[T; 4]> = u8_color.iter().map(|v| v.into_color_comp()).collect();

        self.base.to_rgba(c.as_slice())
    }

    fn components(&self) -> usize {
        1
    }
}

#[derive(Clone, Educe)]
#[educe(Debug, PartialEq)]
pub struct SeparationColorSpace<T: PartialEq + std::fmt::Debug> {
    base: ColorSpace<T>,

    // use Rc, because Box not impl clone trait
    #[educe(Debug(ignore))]
    #[educe(PartialEq(ignore))]
    f: Rc<dyn Function>,
}

impl<T> ColorSpaceTrait<T> for SeparationColorSpace<T>
where
    T: ColorComp + ColorCompConvertTo<f32> + Default + PartialEq + 'static,
    T: ColorCompConvertTo<u8>,
    f32: ColorCompConvertTo<T>,
    u8: ColorCompConvertTo<T>,
{
    fn to_rgba(&self, color: &[T]) -> [T; 4] {
        let c = self.f.call(&[color[0].into_color_comp()]).unwrap();
        let c: ArrayVec<[T; 4]> = c.iter().map(|v| v.into_color_comp()).collect();
        self.base.to_rgba(c.as_slice())
    }

    fn components(&self) -> usize {
        1
    }
}

#[cfg(test)]
mod tests;
