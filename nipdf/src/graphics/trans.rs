//! Lib to translate coordinates. Including CTM,
//! line matrix, pattern etc, and User space to screen space.

use euclid::Transform2D;

pub enum UserSpace {}
/// Coordinate space between UserSpace and DeviceSpace,
/// `ctm` from pdf file, convert User space to Device independent space.
pub enum DeviceIndependentSpace {}
pub enum DeviceSpace {}
// pub enum TextSpace;
pub enum ImageSpace {}
pub type UserToDeviceIndependentSpace = Transform2D<f32, UserSpace, DeviceIndependentSpace>;
// pub enum PatternSpace;
pub type UserToDeviceSpace = Transform2D<f32, UserSpace, DeviceSpace>;
pub type ImageToUserSpace = Transform2D<f32, ImageSpace, UserSpace>;
pub type ImageToDeviceSpace = Transform2D<f32, ImageSpace, DeviceSpace>;

/// Convert current object into tiny_skia `Transform`.
pub trait IntoSkiaTransform {
    fn into_skia(self) -> tiny_skia::Transform;
}

impl<S, D> IntoSkiaTransform for Transform2D<f32, S, D> {
    fn into_skia(self) -> tiny_skia::Transform {
        tiny_skia::Transform::from_row(self.m11, self.m12, self.m21, self.m22, self.m31, self.m32)
    }
}

/// Return a transform convert space to device space.
/// Flip y-axis and apply zoom, because pdf use left-bottom as origin.
pub fn to_device_space<S>(
    device_independent_height: f32,
    zoom: f32,
    to_device_independent: Transform2D<f32, S, DeviceIndependentSpace>,
) -> Transform2D<f32, S, DeviceSpace> {
    to_device_independent
        .with_destination()
        .then_scale(zoom, -zoom)
        .then_translate((0.0, device_independent_height * zoom).into())
}

/// Return a transform from image space to user space.
/// The image (width, height) map to User space (1, 1).
pub fn image_to_user_space(img_w: u32, img_h: u32) -> ImageToUserSpace {
    Transform2D::scale(1.0 / img_w as f32, -1.0 / img_h as f32).then_translate((0.0, 1.0).into())
}

pub fn image_to_device_space(
    img_w: u32,
    img_h: u32,
    device_independent_height: f32,
    zoom: f32,
    ctm: UserToDeviceIndependentSpace,
) -> ImageToDeviceSpace {
    let user = image_to_user_space(img_w, img_h).then(&ctm);
    to_device_space(device_independent_height, zoom, user)
}

#[cfg(test)]
mod tests;
