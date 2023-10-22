pub type RGB = [u8; 3];

pub trait ColorSpace<T, const N: usize> {
    /// Number of color components in this color space.
    const COMPONENTS: usize = N;
    /// Convert color from current space to RGB.
    fn to_rgb(&self, color: [T; N]) -> RGB;
}

#[derive(Debug, Clone, Copy)]
pub struct DeviceGray();

impl ColorSpace<u8, 1> for DeviceGray {
    fn to_rgb(&self, color: [u8; Self::COMPONENTS]) -> RGB {
        [color[0], color[0], color[0]]
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DeviceRGB();

impl ColorSpace<u8, 3> for DeviceRGB {
    fn to_rgb(&self, color: [u8; Self::COMPONENTS]) -> RGB {
        color
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DeviceCMYK();

impl ColorSpace<u8, 4> for DeviceCMYK {
    fn to_rgb(&self, color: [u8; Self::COMPONENTS]) -> RGB {
        let c = to_double(color[0]);
        let m = to_double(color[1]);
        let y = to_double(color[2]);
        let k = to_double(color[3]);
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

        [double_to_u8(r), double_to_u8(g), double_to_u8(b)]
    }
}

fn to_double(v: u8) -> f64 {
    v as f64 / 255.0
}

fn double_to_u8(v: f64) -> u8 {
    (v * 255.0).clamp(0., 255.) as u8
}

#[cfg(test)]
mod tests;
