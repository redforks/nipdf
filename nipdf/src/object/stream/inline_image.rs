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
use prescript::{sname, Name};

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
        self.alt_get(&sname("Width"), &sname("W"), |o| o.int().map(|v| v as u32))?
            .ok_or_else(|| anyhow!("Missing Width"))
    }

    fn height(&self) -> AnyResult<u32> {
        self.alt_get(&sname("Height"), &sname("H"), |o| o.int().map(|v| v as u32))?
            .ok_or_else(|| anyhow!("Missing Height"))
    }

    fn bits_per_component(&self) -> AnyResult<Option<u8>> {
        self.alt_get(&sname("BitsPerComponent"), &sname("BPC"), |o| {
            o.int().map(|v| v as u8)
        })
        .map_err(|e| e.into())
    }

    fn color_space(&self) -> AnyResult<Option<crate::graphics::ColorSpaceArgs>> {
        self.try_from(&sname("ColorSpace"), &sname("CS"))
            .map_err(|e| e.into())
    }

    fn mask(&self) -> AnyResult<Option<super::ImageMask>> {
        Ok(None)
    }

    fn decode(&self) -> AnyResult<Option<crate::function::Domains>> {
        self.try_from(&sname("Decode"), &sname("D"))
            .map_err(|e| e.into())
    }

    fn image_mask(&self) -> AnyResult<bool> {
        Ok(self
            .alt_get(&sname("ImageMask"), &sname("IM"), |o| o.bool())?
            .unwrap_or(false))
    }
}

pub struct InlineStream<'a> {
    d: Dictionary,
    data: &'a [u8],
}

/// Replace abbr name values with standard names.
/// Replace abbr name values with standard names.
fn normalize_name(d: &mut Dictionary) {
    d.update(|d| {
        for (_, v) in d.iter_mut() {
            if let Object::Name(v) = v {
                match v.as_str() {
                    "G" => *v = sname("DeviceGray"),
                    "RGB" => *v = sname("DeviceRGB"),
                    "CMYK" => *v = sname("DeviceCMYK"),
                    "I" => *v = sname("Indexed"),
                    "AHx" => *v = sname("ASCIIHexDecode"),
                    "A85" => *v = sname("ASCII85Decode"),
                    "LZW" => *v = sname("LZWDecode"),
                    "Fl" => *v = sname("FlateDecode"),
                    "RL" => *v = sname("RunLengthDecode"),
                    "CCF" => *v = sname("CCITTFaxDecode"),
                    "DCT" => *v = sname("DCTDecode"),
                    _ => {}
                }
            }
        }
    })
}

impl<'a> InlineStream<'a> {
    pub fn new(mut d: Dictionary, data: &'a [u8]) -> Self {
        normalize_name(&mut d);
        Self { d, data }
    }

    pub fn decode_image(self) -> AnyResult<InlineImage> {
        Ok(InlineImage(self.d, self.data.to_owned()))
    }
}

/// Contains image data and metadata of inlined image.
#[derive(Debug, Clone, PartialEq)]
pub struct InlineImage(Dictionary, Vec<u8>);

impl InlineImage {
    pub(crate) fn meta(&self) -> impl ImageMetadata + '_ {
        InlineStreamDict(&self.0)
    }

    pub fn image(
        &self,
        resolver: &ObjectResolver,
        resources: &ResourceDict,
    ) -> AnyResult<DynamicImage> {
        let decoded_data = decode_stream(&self.0, &self.1, Some(resolver))?;
        Ok(decode_image(
            decoded_data,
            &InlineStreamDict(&self.0),
            resolver,
            Some(resources),
        )?)
    }
}

/// Stub implementation for used in `Operation::PaintInlineImage`,
/// all methods are `unreachable!()`
impl<'b> ConvertFromObject<'b> for InlineImage {
    fn convert_from_object(_objects: &'b mut Vec<Object>) -> Result<Self, ObjectValueError> {
        unreachable!()
    }
}
