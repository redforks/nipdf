//! Lib to translate coordinates. Including CTM,
//! line matrix, pattern etc, and User space to screen space.

use euclid::Transform2D;

pub enum UserSpace {}
pub enum DeviceSpace {}
// pub enum TextSpace;
// pub enum ImageSpace;
// pub enum PatternSpace;
pub type UserToDeviceSpace = Transform2D<f32, UserSpace, DeviceSpace>;

/// Convert current object into tiny_skia `Transform`.
pub trait IntoSkiaTransform {
    fn into_skia(self) -> tiny_skia::Transform;
}

impl<S, D> IntoSkiaTransform for Transform2D<f32, S, D> {
    fn into_skia(self) -> tiny_skia::Transform {
        tiny_skia::Transform::from_row(self.m11, self.m12, self.m21, self.m22, self.m31, self.m32)
    }
}

/// Return a transform from user space to device space.
/// Modify ctm to flip y-axis, because pdf use left-bottom as origin.
pub fn user_to_device_space(height: f32, zoom: f32, ctm: UserToDeviceSpace) -> UserToDeviceSpace {
    ctm.then_scale(zoom, -zoom)
        .then_translate((0.0, height * zoom).into())
}

#[cfg(test)]
mod tests;
