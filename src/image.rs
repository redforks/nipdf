//! Process image stored in stream
use anyhow::Result as AnyResult;
use image::ImageFormat;

use crate::object::Stream;

pub struct Image {
    pub format: ImageFormat,
    pub data: Vec<u8>,
}

pub fn to_image(stream: &Stream) -> AnyResult<Image> {
    todo!()
}
