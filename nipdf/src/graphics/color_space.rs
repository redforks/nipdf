/// Color component composes a color.
/// Two kinds of color component: float or integer.
/// For float color component must in range [0, 1].
pub trait ColorComp: Copy + std::fmt::Debug {
    fn min_color() -> Self;
    /// Max value of color component, for float color component must be 1.0
    fn max_color() -> Self;

    fn range() -> std::ops::RangeInclusive<Self> {
        Self::min_color()..=Self::max_color()
    }
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

pub trait ColorSpaceBoxClone<T> {
    fn clone_box(&self) -> Box<dyn ColorSpace<T>>;
}

impl<T, O: Clone + ColorSpace<T> + 'static> ColorSpaceBoxClone<T> for O {
    fn clone_box(&self) -> Box<dyn ColorSpace<T>> {
        Box::new(self.clone())
    }
}

/// Convert color to rgba color space, convert result to f32 or u8 by T generic type.
pub fn color_to_rgba<F, T, CS>(cs: CS, color: &[F]) -> [T; 4]
where
    F: ColorComp,
    T: ColorComp,
    CS: ColorSpace<F>,
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

pub trait ColorSpace<T>: std::fmt::Debug + ColorSpaceBoxClone<T> {
    /// Convert color from current space to RGBA.
    /// `color` len should at least be `components()`
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

impl<T> Clone for Box<dyn ColorSpace<T>> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DeviceGray();

impl<T: ColorComp> ColorSpace<T> for DeviceGray {
    fn to_rgba(&self, color: &[T]) -> [T; 4] {
        [color[0], color[0], color[0], T::max_color()]
    }

    fn components(&self) -> usize {
        1
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DeviceRGB();

impl<T: ColorComp> ColorSpace<T> for DeviceRGB {
    fn to_rgba(&self, color: &[T]) -> [T; 4] {
        [color[0], color[1], color[2], T::max_color()]
    }

    fn components(&self) -> usize {
        3
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DeviceCMYK();

impl<T> ColorSpace<T> for DeviceCMYK
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

impl<T> ColorSpace<T> for PatternColorSpace {
    fn to_rgba(&self, _color: &[T]) -> [T; 4] {
        unreachable!("PatternColorSpace.to_rgba() should not be called")
    }

    fn components(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests;
