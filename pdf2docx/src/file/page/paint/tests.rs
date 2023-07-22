use super::*;
use test_case::test_case;

#[test_case(Color::Rgb(0.1, 0.2, 0.3) => tiny_skia::Color::from_rgba(0.1, 0.2, 0.3, 1.0).unwrap(); "rgb")]
#[test_case(Color::Cmyk(0.0, 0.0, 0.0, 1.0) => tiny_skia::Color::from_rgba(0.0, 0.0, 0.0, 1.0).unwrap(); "cmyk")]
#[test_case(Color::Gray(0.5) => tiny_skia::Color::from_rgba(0.5, 0.5, 0.5, 1.0).unwrap(); "gray")]
fn test_convert_color_to_skia_color(c: Color) -> tiny_skia::Color {
    tiny_skia::Color::from(c)
}
