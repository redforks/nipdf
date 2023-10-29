use super::*;
use assert_approx_eq::assert_approx_eq;

#[test]
fn device_gray_to_rgb() {
    let color_space = DeviceGray();
    let rgba = color_space.to_rgba(&[0x80]);
    assert_eq!(rgba, [0x80, 0x80, 0x80, 0xff]);
}

#[test]
fn rgb_to_rgb() {
    let color_space = DeviceRGB();
    let color = [0x1, 0x2, 0x3];
    let rgba = color_space.to_rgba(&color);
    assert_eq!(rgba, [1, 2, 3, 255]);
}

#[test]
fn cmyk_to_rgb() {
    let color_space = DeviceCMYK();
    let color = [0, 0, 0, 0];
    let rgb = color_space.to_rgba(&color);
    assert_eq!(rgb, [255, 255, 255, 255]);

    let color = [255, 0, 0, 0];
    let rgb = color_space.to_rgba(&color);
    assert_eq!(rgb, [0, 173, 239, 255]);
}

#[test]
fn convert_color_comp_u8_to_f32() {
    assert_eq!(0.0f32, 0_u8.into_color_comp());
    assert_eq!(1.0f32, 255_u8.into_color_comp());
    assert_approx_eq!(
        0.5f32,
        ColorCompConvertTo::<f32>::into_color_comp(127_u8),
        0.01
    );
}

#[test]
fn convert_color_com_f32_to_u8() {
    assert_eq!(0_u8, 0.0f32.into_color_comp());
    assert_eq!(255_u8, 1.0f32.into_color_comp());
    assert_eq!(128_u8, 0.5f32.into_color_comp()); // round integer part
    assert_eq!(0_u8, ColorCompConvertTo::<u8>::into_color_comp(-1.0f32));
    assert_eq!(255_u8, 33f32.into_color_comp());
}

#[test]
fn test_color_to_rgba() {
    // DeviceGray u8 to u8 rgba
    let color_space = DeviceGray();
    assert_eq!(
        color_to_rgba::<_, u8, _>(color_space, &[0x80]),
        [0x80, 0x80, 0x80, 0xff]
    );

    // DeviceGray u8 to f32 rgba
    let color_space = DeviceGray();
    assert_eq!(
        color_to_rgba::<_, f32, _>(color_space, &[51]),
        [0.2f32, 0.2f32, 0.2f32, 1.0f32]
    );
}

#[test]
fn indexed_color_space() {
    let color_space = IndexedColorSpace {
        base: Box::new(DeviceRGB()),
        data: vec![
            0x00, 0x00, 0x00, // black
            0xff, 0xff, 0xff, // white
        ],
    };
    assert_eq!(2, color_space.len());
    assert_eq!(color_space.to_rgba(&[0]), [0, 0, 0, 255]);
    assert_eq!(color_space.to_rgba(&[1]), [255, 255, 255, 255]);
}
