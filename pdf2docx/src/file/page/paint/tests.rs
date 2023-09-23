use super::*;
use test_case::test_case;
use tiny_skia::Point;

#[test_case(Color::Rgb(0.1, 0.2, 0.3) => tiny_skia::Color::from_rgba(0.1, 0.2, 0.3, 1.0).unwrap(); "rgb")]
#[test_case(Color::Cmyk(0.0, 0.0, 0.0, 1.0) => tiny_skia::Color::from_rgba(0.0, 0.0, 0.0, 1.0).unwrap(); "cmyk")]
#[test_case(Color::Gray(0.5) => tiny_skia::Color::from_rgba(0.5, 0.5, 0.5, 1.0).unwrap(); "gray")]
fn test_convert_color_to_skia_color(c: Color) -> tiny_skia::Color {
    tiny_skia::Color::from(c)
}

fn map_point_asserter(m: Transform) -> impl Fn((f32, f32), (f32, f32)) {
    move |p, exp| {
        let mut p = Point::from_xy(p.0, p.1);
        m.map_point(&mut p);
        assert!(
            (exp.0 - p.x).abs() < 0.0001 && (exp.1 - p.y).abs() < 0.0001,
            "({}, {}) != ({}, {})",
            exp.0,
            exp.1,
            p.x,
            p.y
        );
    }
}

#[test]
fn path_transform() {
    let m = MatrixMapper::new(100., 600.0, 1.0, TransformMatrix::identity());
    let assert_mp = map_point_asserter(m.path_transform());
    assert_mp((10.0, 20.0), (10.0, 600.0 - 20.0));
    assert_mp((10.0, 1.0), (10.0, 599.0));

    // zoom 1.5
    let m = MatrixMapper::new(100., 600.0 * 1.5, 1.5, TransformMatrix::identity());
    let assert_mp = map_point_asserter(m.path_transform());
    assert_mp((10.0, 20.0), (15.0, 600.0 * 1.5 - 20.0 * 1.5));
    assert_mp((10.0, 1.0), (15.0, 600.0 * 1.5 - 1.0 * 1.5));
    assert_mp((10.0, 0.0), (15.0, 600.0 * 1.5));
    assert_mp((10.0, 600.0), (15.0, 0.0));
}

#[test_case(1.0, (54.24, 510.96))]
#[test_case(1.5, (81.36, 792.0*1.5 - 279.12*1.5 - 1.92*1.5))]
fn image_transform(zoom: f32, exp: (f32, f32)) {
    let m = MatrixMapper::new(
        10.,
        792.0 * zoom,
        zoom,
        TransformMatrix {
            sx: 468.48,
            kx: 0.0,
            ky: 0.0,
            sy: 1.92,
            tx: 54.24,
            ty: 279.12,
        },
    );
    let assert_mp = map_point_asserter(m.image_transform(1301, 5));
    assert_mp((0.0, 0.0), exp);
}

#[test]
fn font_width_empty() {
    let font_width = TrueTypeFontWidth {
        range: None,
        widths: vec![],
        default_width: 15,
    };

    assert_eq!(15, font_width.char_width('f' as u32));
    assert_eq!(15, font_width.char_width('g' as u32));
}

#[test]
fn font_width() {
    let font_width = TrueTypeFontWidth {
        range: Some('a' as u32..='d' as u32),
        widths: vec![100, 200, 300, 400],
        default_width: 15,
    };

    assert_eq!(100, font_width.char_width('a' as u32));
    assert_eq!(200, font_width.char_width('b' as u32));
    assert_eq!(400, font_width.char_width('d' as u32));
    assert_eq!(15, font_width.char_width('e' as u32));
}
