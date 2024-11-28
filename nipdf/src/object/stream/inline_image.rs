//! Inline Image and Inline Image Stream
//!
//! InlineImage decode from InlineImageStream
use super::{ImageMetadata, decode_image, decode_stream};
use crate::{
    Result,
    file::{ObjectResolver, ResourceDict},
    graphics::ConvertFromObject,
    object::{Dictionary, Object, ObjectValueError},
};
use image::DynamicImage;
use prescript::{Name, sname};
use snafu::{OptionExt, ResultExt};

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

impl ImageMetadata for InlineStreamDict<'_> {
    fn width(&self) -> Result<u32> {
        self.alt_get(&sname("Width"), &sname("W"), |o| o.int().map(|v| v as u32))
            .whatever_context("get width")?
            .whatever_context("Missing Width")
    }

    fn height(&self) -> Result<u32> {
        self.alt_get(&sname("Height"), &sname("H"), |o| o.int().map(|v| v as u32))
            .whatever_context("get height")?
            .whatever_context("Missing Height")
    }

    fn bits_per_component(&self) -> Result<Option<u8>> {
        self.alt_get(&sname("BitsPerComponent"), &sname("BPC"), |o| {
            o.int().map(|v| v.try_into().unwrap())
        })
        .whatever_context("get BitsPerComponent")
    }

    fn color_space(&self) -> Result<Option<crate::graphics::ColorSpaceArgs>> {
        self.try_from(&sname("ColorSpace"), &sname("CS"))
            .whatever_context("get ColorSpace")
    }

    fn mask(&self) -> Result<Option<super::ImageMask>> {
        Ok(None)
    }

    fn decode(&self) -> Result<Option<crate::function::Domains>> {
        self.try_from(&sname("Decode"), &sname("D"))
            .whatever_context("get Decode")
    }

    fn image_mask(&self) -> Result<bool> {
        Ok(self
            .alt_get(&sname("ImageMask"), &sname("IM"), Object::bool)
            .whatever_context("get ImageMask")?
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
    fn replace_name(name: &mut Name) {
        match name.as_str() {
            "G" => *name = sname("DeviceGray"),
            "RGB" => *name = sname("DeviceRGB"),
            "CMYK" => *name = sname("DeviceCMYK"),
            "I" => *name = sname("Indexed"),
            "AHx" => *name = sname("ASCIIHexDecode"),
            "A85" => *name = sname("ASCII85Decode"),
            "LZW" => *name = sname("LZWDecode"),
            "Fl" => *name = sname("FlateDecode"),
            "RL" => *name = sname("RunLengthDecode"),
            "CCF" => *name = sname("CCITTFaxDecode"),
            "DCT" => *name = sname("DCTDecode"),
            _ => {}
        }
    }

    d.update(|d| {
        for (_, v) in d.iter_mut() {
            match v {
                Object::Name(v) => replace_name(v),
                Object::Array(arr) => {
                    Object::update_array_items(arr, |v| {
                        if let Object::Name(v) = v {
                            replace_name(v);
                        }
                    });
                }
                _ => {}
            }
        }
    });
}

impl<'a> InlineStream<'a> {
    pub fn new(mut d: Dictionary, data: &'a [u8]) -> Self {
        normalize_name(&mut d);
        Self { d, data }
    }

    pub fn decode_image(self) -> Result<InlineImage> {
        Ok(InlineImage(self.d, self.data.to_owned()))
    }
}

/// Contains image data and metadata of inlined image.
#[derive(Debug, Clone, PartialEq)]
pub struct InlineImage(Dictionary, Vec<u8>);

impl InlineImage {
    pub fn meta(&self) -> impl ImageMetadata + '_ {
        InlineStreamDict(&self.0)
    }

    pub fn image(
        &self,
        resolver: &ObjectResolver<'_>,
        resources: &ResourceDict<'_, '_>,
    ) -> Result<DynamicImage> {
        let decoded_data = decode_stream(&self.0, &self.1, Some(resolver), None, None)
            .whatever_context("decode stream")?;
        decode_image(
            decoded_data,
            &InlineStreamDict(&self.0),
            resolver,
            Some(resources),
        )
        .whatever_context("decode image")
    }
}

/// Stub implementation for used in `Operation::PaintInlineImage`,
/// all methods are `unreachable!()`
impl<'b> ConvertFromObject<'b> for InlineImage {
    fn convert_from_object(_objects: &'b mut Vec<Object>) -> Result<Self, ObjectValueError> {
        unreachable!()
    }
}
