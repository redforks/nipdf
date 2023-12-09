use nipdf::graphics::Point;

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
