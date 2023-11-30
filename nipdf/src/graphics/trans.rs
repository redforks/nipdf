//! Lib to translate coordinates. Including CTM,
//! line matrix, pattern etc, and User space to screen space.

use euclid::Transform2D;

pub enum UserSpace {}
/// Coordinate space between UserSpace and DeviceSpace,
/// `ctm` from pdf file, convert User space to Device independent space.
pub enum LogicDeviceSpace {}
pub enum DeviceSpace {}
pub enum ImageSpace {}
pub enum TextSpace {}
pub enum FormSpace {}
pub type UserToLogicDeviceSpace = Transform2D<f32, UserSpace, LogicDeviceSpace>;
// pub enum PatternSpace;
pub type UserToDeviceSpace = Transform2D<f32, UserSpace, DeviceSpace>;
pub type ImageToUserSpace = Transform2D<f32, ImageSpace, UserSpace>;
pub type ImageToDeviceSpace = Transform2D<f32, ImageSpace, DeviceSpace>;
pub type TextToUserSpace = Transform2D<f32, TextSpace, UserSpace>;
pub type FormToUserSpace = Transform2D<f32, FormSpace, UserSpace>;

/// Convert current object into tiny_skia `Transform`.
pub trait IntoSkiaTransform {
    fn into_skia(self) -> tiny_skia::Transform;
}

impl<S, D> IntoSkiaTransform for Transform2D<f32, S, D> {
    fn into_skia(self) -> tiny_skia::Transform {
        tiny_skia::Transform::from_row(self.m11, self.m12, self.m21, self.m22, self.m31, self.m32)
    }
}

pub fn logic_device_to_device(
    logic_device_height: f32,
    zoom: f32,
) -> Transform2D<f32, LogicDeviceSpace, DeviceSpace> {
    Transform2D::scale(zoom, -zoom).then_translate((0.0, logic_device_height * zoom).into())
}

/// Return a transform convert space to device space.
/// Flip y-axis and apply zoom, because pdf use left-bottom as origin.
pub fn to_device_space<S>(
    logic_device_height: f32,
    zoom: f32,
    to_logic_device: &Transform2D<f32, S, LogicDeviceSpace>,
) -> Transform2D<f32, S, DeviceSpace> {
    to_logic_device.then(&logic_device_to_device(logic_device_height, zoom))
}

/// Return a transform from image space to user space.
/// The image (width, height) map to User space (1, 1).
pub fn image_to_user_space(img_w: u32, img_h: u32) -> ImageToUserSpace {
    Transform2D::scale(1.0 / img_w as f32, -1.0 / img_h as f32).then_translate((0.0, 1.0).into())
}

pub fn image_to_device_space(
    img_w: u32,
    img_h: u32,
    logic_device_height: f32,
    zoom: f32,
    ctm: &UserToLogicDeviceSpace,
) -> ImageToDeviceSpace {
    image_to_user_space(img_w, img_h)
        .then(ctm)
        .then(&logic_device_to_device(logic_device_height, zoom))
}

/// Adjust transform moves text space to right.
pub fn move_text_space_right(transform: &TextToUserSpace, x_text_space: f32) -> TextToUserSpace {
    move_text_space_pos(transform, x_text_space, 0.)
}

/// Adjust transform to moves position in text space.
pub fn move_text_space_pos(
    transform: &TextToUserSpace,
    x_text_space: f32,
    y_text_space: f32,
) -> TextToUserSpace {
    transform.pre_translate((x_text_space, y_text_space).into())
}

#[cfg(test)]
mod tests;
