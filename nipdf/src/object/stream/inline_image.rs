//! Inline Image and Inline Image Stream
//!
//! InlineImage decode from InlineImageStream
use super::{decode_image, decode_stream, AnyResult, ImageMetadata};
use crate::{
    file::{ObjectResolver, ResourceDict},
    graphics::ConvertFromObject,
    object::{Dictionary, Object, ObjectValueError},
};
use anyhow::anyhow;
use image::DynamicImage;
use prescript::{name, Name};

struct InlineStreamDict<'a>(&'a Dictionary);

impl<'a> InlineStreamDict<'a> {
    fn alt_get<T>(
        &self,
        id1: &Name,
        id2: &Name,
        f: impl Fn(&'a Object) -> Result<T, ObjectValueError>,
    ) -> Result<Option<T>, ObjectValueError> {
        self.0
            .get(id1)
            .or_else(|| self.0.get(id2))
            .map(f)
            .transpose()
    }

    fn try_from<T: TryFrom<&'a Object, Error = ObjectValueError>>(
        &self,
        id1: &Name,
        id2: &Name,
    ) -> Result<Option<T>, ObjectValueError> {
        self.alt_get(id1, id2, T::try_from)
    }
}

impl<'a> ImageMetadata for InlineStreamDict<'a> {
    fn width(&self) -> AnyResult<u32> {
        self.alt_get(&name!("Width"), &name!("W"), |o| o.int().map(|v| v as u32))?
            .ok_or_else(|| anyhow!("Missing Width"))
    }

    fn height(&self) -> AnyResult<u32> {
        self.alt_get(&name!("Height"), &name!("H"), |o| o.int().map(|v| v as u32))?
            .ok_or_else(|| anyhow!("Missing Height"))
    }

    fn bits_per_component(&self) -> AnyResult<Option<u8>> {
        self.alt_get(&name!("BitsPerComponent"), &name!("BPC"), |o| {
            o.int().map(|v| v as u8)
        })
        .map_err(|e| e.into())
    }

    fn color_space(&self) -> AnyResult<Option<crate::graphics::ColorSpaceArgs>> {
        self.try_from(&name!("ColorSpace"), &name!("CS"))
            .map_err(|e| e.into())
    }

    fn mask(&self) -> AnyResult<Option<super::ImageMask>> {
        self.try_from(&name!("ImageMask"), &name!("IM"))
            .map_err(|e| e.into())
    }

    fn decode(&self) -> AnyResult<Option<crate::function::Domains>> {
        self.try_from(&name!("Decode"), &name!("D"))
            .map_err(|e| e.into())
    }
}

pub struct InlineStream<'a> {
    d: Dictionary,
    data: &'a [u8],
}

impl<'a> InlineStream<'a> {
    pub fn new(d: Dictionary, data: &'a [u8]) -> Self {
        Self { d, data }
    }

    pub fn decode_image(self) -> AnyResult<InlineImage> {
        let decoded_data = decode_stream(&self.d, self.data, None)?;
        Ok(InlineImage(self.d, decoded_data.into_bytes()?.into_owned()))
    }
}

/// Contains image data and metadata of inlined image.
#[derive(Debug, Clone, PartialEq)]
pub struct InlineImage(Dictionary, Vec<u8>);

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
