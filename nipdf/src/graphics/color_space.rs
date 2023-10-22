/// Color component composes a color.
/// Two kinds of color component: float or integer.
/// For float color component must in range [0, 1].
pub trait ColorComp: Copy {
    fn to_f32_color_comp(self) -> f32;
    fn to_u8_color_comp(self) -> u8;

    fn from_f32_color(v: f32) -> Self;

    fn min_color() -> Self;
    /// Max value of color component, for float color component must be 1.0
    fn max_color() -> Self;

    fn range() -> std::ops::RangeInclusive<Self> {
        Self::min_color()..=Self::max_color()
    }
}

impl ColorComp for u8 {
    fn to_f32_color_comp(self) -> f32 {
        self as f32 / 255.0
    }

    fn to_u8_color_comp(self) -> u8 {
        self
    }

    fn from_f32_color(v: f32) -> Self {
        (v * 255.0).clamp(0., 255.) as u8
    }

    fn min_color() -> Self {
        0
    }

    fn max_color() -> Self {
        255
    }
}

impl ColorComp for f32 {
    fn to_f32_color_comp(self) -> f32 {
        self
    }

    fn to_u8_color_comp(self) -> u8 {
        (self * 255.0).clamp(0., 255.) as u8
    }

    fn from_f32_color(v: f32) -> Self {
        debug_assert!((0.0f32..1.0f32).contains(&v));
        v
    }

    fn min_color() -> Self {
        0.0
    }

    fn max_color() -> Self {
        1.0
    }
}

pub trait ToF32Color<T, const N: usize> {
    fn to_f32_color(self) -> [f32; N];
}

impl<T: ColorComp, const N: usize> ToF32Color<T, N> for [T; N] {
    fn to_f32_color(self) -> [f32; N] {
        let mut color = [0.; N];
        for i in 0..N {
            color[i] = self[i].to_f32_color_comp();
        }
        color
    }
}

pub trait ToU8Color<T, const N: usize> {
    fn to_u8_color(self) -> [u8; N];
}

impl<T: ColorComp, const N: usize> ToU8Color<T, N> for [T; N] {
    fn to_u8_color(self) -> [u8; N] {
        let mut color = [0; N];
        for i in 0..N {
            color[i] = self[i].to_u8_color_comp();
        }
        color
    }
}

pub trait ColorSpace<T> {
    /// Convert color from current space to RGBA.
    /// `color` len should at least be `components()`
    fn to_rgba(&self, color: &[T]) -> [T; 4];

    /// Number of color components in this color space.
    fn components(&self) -> usize;

    /// Convert color from current space to RGBA tiny_skia color
    /// `color` len should at least be `components()`
    fn to_skia_color(&self, color: &[T]) -> tiny_skia::Color
    where
        T: ColorComp,
    {
        let rgba = self.to_rgba(color);
        tiny_skia::Color::from_rgba(
            rgba[0].to_f32_color_comp(),
            rgba[1].to_f32_color_comp(),
            rgba[2].to_f32_color_comp(),
            rgba[3].to_f32_color_comp(),
        )
        .unwrap()
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

impl<T: ColorComp> ColorSpace<T> for DeviceCMYK {
    fn to_rgba(&self, color: &[T]) -> [T; 4] {
        let c = color[0].to_f32_color_comp();
        let m = color[1].to_f32_color_comp();
        let y = color[2].to_f32_color_comp();
        let k = color[3].to_f32_color_comp();
        let c1 = 1.0 - c;
        let m1 = 1.0 - m;
        let y1 = 1.0 - y;
        let k1 = 1.0 - k;

        let x = c1 * m1 * y1 * k1;
        let (mut r, mut g, mut b) = (x, x, x);
        r += 0.1373 * x;
        g += 0.1216 * x;
        b += 0.1255 * x;

        let x = c * m1 * y * k1;
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
            T::from_f32_color(r),
            T::from_f32_color(g),
            T::from_f32_color(b),
            T::max_color(),
        ]
    }

    fn components(&self) -> usize {
        4
    }
}

#[cfg(test)]
mod tests;
