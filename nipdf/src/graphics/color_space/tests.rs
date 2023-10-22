use super::*;

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
    assert_eq!(rgb, [0, 172, 239, 255]);
}
