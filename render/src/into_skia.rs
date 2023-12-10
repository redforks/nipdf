use nipdf::{
    file::Rectangle,
    graphics::{LineCapStyle, LineJoinStyle, Point},
};
use tiny_skia::LineJoin;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangle_to_skia() {
        let rect = Rectangle::from_xywh(98.0, 519.0, 423.0, -399.0);
        let skia_rect: tiny_skia::Rect = rect.into_skia();
        assert_eq!(
            skia_rect,
            tiny_skia::Rect::from_ltrb(98.0, 519.0 - 399.0, 98.0 + 423.0, 519.0).unwrap()
        );
    }
}
