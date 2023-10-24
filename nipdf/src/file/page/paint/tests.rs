use super::*;
use test_case::test_case;
use tiny_skia::Point;

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
fn first_last_font_width() {
    let font_width = FirstLastFontWidth {
        range: 'a' as u32..='d' as u32,
        widths: vec![100, 200, 300, 400],
        default_width: 15,
    };

    assert_eq!(100, font_width.char_width('a' as u32));
    assert_eq!(200, font_width.char_width('b' as u32));
    assert_eq!(400, font_width.char_width('d' as u32));
    assert_eq!(15, font_width.char_width('e' as u32));
}
