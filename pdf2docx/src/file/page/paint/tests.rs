use super::*;
use test_case::test_case;
use tiny_skia::Point;

#[test_case(Color::Rgb(0.1, 0.2, 0.3) => tiny_skia::Color::from_rgba(0.1, 0.2, 0.3, 1.0).unwrap(); "rgb")]
#[test_case(Color::Cmyk(0.0, 0.0, 0.0, 1.0) => tiny_skia::Color::from_rgba(0.0, 0.0, 0.0, 1.0).unwrap(); "cmyk")]
#[test_case(Color::Gray(0.5) => tiny_skia::Color::from_rgba(0.5, 0.5, 0.5, 1.0).unwrap(); "gray")]
fn test_convert_color_to_skia_color(c: Color) -> tiny_skia::Color {
    tiny_skia::Color::from(c)
}

#[test]
fn path_transform() {
    let m = MatrixMapper::new(600, TransformMatrix::identity());
    let t = m.path_transform();

    let mut p = Point::from_xy(10.0, 20.0);
    t.map_point(&mut p);
    assert_eq!(p, Point::from_xy(10.0, 600.0 - 20.0));

    let mut p = Point::from_xy(10.0, 1.0);
    t.map_point(&mut p);
    assert_eq!(p, Point::from_xy(10.0, 599.0));
}

/// assert that two points are almost equal
fn assert_point(a: Point, b: Point) {
    assert!(
        (a.x - b.x).abs() < 0.0001 && (a.y - b.y).abs() < 0.0001,
        "({}, {}) != ({}, {})",
        a.x,
        a.y,
        b.x,
        b.y
    );
}

#[test]
fn image_transform() {
    let m = MatrixMapper::new(
        792,
        TransformMatrix {
            sx: 468.48,
            kx: 0.0,
            ky: 0.0,
            sy: 1.92,
            tx: 54.24,
            ty: 279.12,
        },
    );
    let t = m.image_transform(1301, 5);

    let mut p = Point::zero();
    t.map_point(&mut p);
    assert_point(p, Point::from_xy(54.24, 510.96));
}
