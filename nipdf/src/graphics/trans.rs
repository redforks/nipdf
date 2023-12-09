//! Lib to translate coordinates. Including CTM,
//! line matrix, pattern etc, and User space to screen space.

use euclid::{Length, Point2D, Transform2D};
use num::traits::AsPrimitive;

pub enum UserSpace {}
/// Coordinate space between UserSpace and DeviceSpace,
/// `ctm` from pdf file, convert User space to Device independent space.
pub enum LogicDeviceSpace {}
pub enum DeviceSpace {}
pub enum ImageSpace {}
pub enum TextSpace {}
pub enum FormSpace {}
pub enum PatternSpace {}
pub enum GlyphSpace {}
pub enum ThousandthsOfText {}
pub type GlyphLength = Length<f32, GlyphSpace>;
pub type TextPoint = Point2D<f32, TextSpace>;
pub type GlyphToTextSpace = Transform2D<f32, GlyphSpace, TextSpace>;
pub type GlyphToUserSpace = Transform2D<f32, GlyphSpace, UserSpace>;
pub type UserToUserSpace = Transform2D<f32, UserSpace, UserSpace>;
pub type UserToLogicDeviceSpace = Transform2D<f32, UserSpace, LogicDeviceSpace>;
pub type UserToDeviceSpace = Transform2D<f32, UserSpace, DeviceSpace>;
pub type LogicDeviceToDeviceSpace = Transform2D<f32, LogicDeviceSpace, DeviceSpace>;
pub type ImageToUserSpace = Transform2D<f32, ImageSpace, UserSpace>;
pub type ImageToDeviceSpace = Transform2D<f32, ImageSpace, DeviceSpace>;
pub type TextToUserSpace = Transform2D<f32, TextSpace, UserSpace>;
pub type FormToUserSpace = Transform2D<f32, FormSpace, UserSpace>;
pub type PatternToUserSpace = Transform2D<f32, PatternSpace, UserSpace>;

/// Convert current object into tiny_skia `Transform`.
pub trait IntoSkiaTransform {
    fn into_skia(self) -> tiny_skia::Transform;
}

impl<S, D> IntoSkiaTransform for Transform2D<f32, S, D> {
    fn into_skia(self) -> tiny_skia::Transform {
        tiny_skia::Transform::from_row(self.m11, self.m12, self.m21, self.m22, self.m31, self.m32)
    }
}

pub fn f_flip<S, D>(height: f32) -> Transform2D<f32, S, D> {
    Transform2D::scale(1.0, -1.0).then_translate((0.0, height).into())
}

pub fn logic_device_to_device(
    logic_device_height: impl AsPrimitive<f32>,
    zoom: f32,
) -> Transform2D<f32, LogicDeviceSpace, DeviceSpace> {
    Transform2D::scale(zoom, -zoom).then_translate((0.0, logic_device_height.as_() * zoom).into())
}

/// Return a transform from image space to user space.
/// The image (width, height) map to User space (1, 1).
pub fn image_to_user_space(img_w: u32, img_h: u32) -> ImageToUserSpace {
    Transform2D::scale(1.0 / img_w as f32, -1.0 / img_h as f32).then_translate((0.0, 1.0).into())
}

/// Adjust transform moves text space to right.
pub fn move_text_space_right(
    transform: &TextToUserSpace,
    x_text_space: Length<f32, TextSpace>,
) -> TextToUserSpace {
    move_text_space_pos(transform, TextPoint::new(x_text_space.0, 0.0))
}

/// Adjust transform to moves position in text space.
pub fn move_text_space_pos(transform: &TextToUserSpace, p: TextPoint) -> TextToUserSpace {
    transform.pre_translate(p.to_vector())
}

#[cfg(test)]
mod tests;
