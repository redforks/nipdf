use super::*;

#[test]
fn device_gray_to_rgb() {
    let color_space = DeviceGray();
    let color = [0x80];
    let rgb = color_space.to_rgb(color);
    assert_eq!(rgb, [0x80, 0x80, 0x80]);
}

#[test]
fn rgb_to_rgb() {
    let color_space = DeviceRGB();
    let color = [0x80, 0x80, 0x80];
    let rgb = color_space.to_rgb(color);
    assert_eq!(rgb, [0x80, 0x80, 0x80]);
}

#[test]
fn cmyk_to_rgb() {
    let color_space = DeviceCMYK();
    let color = [0, 0, 0, 0];
    let rgb = color_space.to_rgb(color);
    assert_eq!(rgb, [255, 255, 255]);

    let color = [255, 0, 0, 0];
    let rgb = color_space.to_rgb(color);
    assert_eq!(rgb, [0, 172, 239]);
}
