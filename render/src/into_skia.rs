use euclid::Transform2D;
use nipdf::{
    file::Rectangle,
    graphics::{
        color_space::{convert_color_to, ColorComp, ColorCompConvertTo, ColorSpaceTrait},
        LineCapStyle, LineJoinStyle, Point,
    },
};

pub trait IntoSkia {
    type Output;
    fn into_skia(self) -> Self::Output;
}

impl IntoSkia for Point {
    type Output = tiny_skia::Point;

    fn into_skia(self) -> Self::Output {
        Self::Output::from_xy(self.x, self.y)
    }
}

impl IntoSkia for Rectangle {
    type Output = tiny_skia::Rect;

    fn into_skia(self) -> Self::Output {
        Self::Output::from_ltrb(self.left_x, self.lower_y, self.right_x, self.upper_y).unwrap()
    }
}

impl IntoSkia for LineCapStyle {
    type Output = tiny_skia::LineCap;

    fn into_skia(self) -> Self::Output {
        match self {
            Self::Butt => Self::Output::Butt,
            Self::Round => Self::Output::Round,
            Self::Square => Self::Output::Square,
        }
    }
}

impl IntoSkia for LineJoinStyle {
    type Output = tiny_skia::LineJoin;

    fn into_skia(self) -> Self::Output {
        match self {
            Self::Miter => Self::Output::Miter,
            Self::Round => Self::Output::Round,
            Self::Bevel => Self::Output::Bevel,
        }
    }
}

impl<S, D> IntoSkia for Transform2D<f32, S, D> {
    type Output = tiny_skia::Transform;

    fn into_skia(self) -> Self::Output {
        tiny_skia::Transform::from_row(self.m11, self.m12, self.m21, self.m22, self.m31, self.m32)
    }
}

pub fn to_skia_color<T>(cs: &impl ColorSpaceTrait<T>, color: &[T]) -> tiny_skia::Color
where
    T: ColorComp + ColorCompConvertTo<f32>,
{
    let rgba = cs.to_rgba(color);
    let [r, g, b, a] = convert_color_to(&rgba);
    tiny_skia::Color::from_rgba(r, g, b, a).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use euclid::default::Transform2D;

    #[test]
    fn rectangle_to_skia() {
        let rect = Rectangle::from_xywh(98.0, 519.0, 423.0, -399.0);
        let skia_rect: tiny_skia::Rect = rect.into_skia();
        assert_eq!(
            skia_rect,
            tiny_skia::Rect::from_ltrb(98.0, 519.0 - 399.0, 98.0 + 423.0, 519.0).unwrap()
        );
    }

    #[test]
    fn transform_to_skia() {
        let m = Transform2D::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
        let skia = m.into_skia();
        assert_eq!(skia.sx, 1.0);
        assert_eq!(skia.ky, 2.0);
        assert_eq!(skia.kx, 3.0);
        assert_eq!(skia.sy, 4.0);
        assert_eq!(skia.tx, 5.0);
        assert_eq!(skia.ty, 6.0);
    }
}
