use super::*;

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
