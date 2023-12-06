//! Inline Image and Inline Image Stream
//!
//! InlineImage decode from InlineImageStream
use super::AnyResult;
use crate::{
    graphics::ConvertFromObject,
    object::{Dictionary, Object, ObjectValueError},
};

pub struct InlineStream<'a> {
    d: Dictionary,
    data: &'a [u8],
}

impl<'a> InlineStream<'a> {
    pub fn new(d: Dictionary, data: &'a [u8]) -> Self {
        Self { d, data }
    }

    pub fn decode_image(&self) -> AnyResult<InlineImage> {
        todo!()
    }
}

/// Contains image data and metadata of inlined image.
#[derive(Debug, Clone, PartialEq)]
pub struct InlineImage;

/// Stub implementation for used in `Operation::PaintInlineImage`,
/// all methods are `unreachable!()`
impl<'b> ConvertFromObject<'b> for InlineImage {
    fn convert_from_object(_objects: &'b mut Vec<Object>) -> Result<Self, ObjectValueError> {
        unreachable!()
    }
}

/*
pub struct InlineImage {
    width: u32,
    height: u32,
    bits_per_component: u8,
    color_space: ColorSpaceArgs,
    image_mask: bool,
    interpolate: bool,
}

impl InlineImage {
    fn from(d: &Dictionary) -> Result<Self, ObjectValueError> {
        let d = SchemaDict::new(d, &(), ())?;
        let width = d
            .opt_u32(name!("Width"))
            .transpose()
            .or_else(|| d.opt_u32(name!("W")).transpose())
            .transpose()?
            .ok_or(ObjectValueError::GraphicsOperationSchemaError)?;
        Ok(Self {
            width,
            height: height as u32,
            bits_per_component,
            color_space,
            image_mask,
            interpolate,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn bits_per_component(&self) -> u8 {
        self.bits_per_component
    }

    pub fn color_space(&self) -> ColorSpaceArgs {
        self.color_space
    }

    pub fn image_mask(&self) -> bool {
        self.image_mask
    }

    pub fn interpolate(&self) -> bool {
        self.interpolate
    }
}

*/
