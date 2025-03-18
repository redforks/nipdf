//! Inline Image and Inline Image Stream
//!
//! InlineImage decode from InlineImageStream
use super::{ImageMetadata, decode_image, decode_inline_stream};
use crate::{
    Result,
    file::{ObjectResolver, ResourceDict},
    graphics::ConvertFromObject,
    object::{Dictionary, Object, ObjectValueError},
};
use educe::Educe;
use image::RgbaImage;
use once_map::OnceMap;
use prescript::{Name, sname};
use snafu::{OptionExt, ResultExt};
use std::{
    hash::{Hash, Hasher},
    rc::Rc,
};

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
        self.alt_get(&sname("W"), &sname("Width"), |o| o.int().map(|v| v as u32))
            .whatever_context::<_, ObjectValueError>("get width")?
            .whatever_context::<_, ObjectValueError>("Missing Width")
    }

    fn height(&self) -> Result<u32> {
        self.alt_get(&sname("H"), &sname("Height"), |o| o.int().map(|v| v as u32))
            .whatever_context::<_, ObjectValueError>("get height")?
            .whatever_context::<_, ObjectValueError>("Missing Height")
    }

    fn bits_per_component(&self) -> Result<Option<u8>> {
        self.alt_get(&sname("BPC"), &sname("BitsPerComponent"), |o| {
            o.int()
                .and_then(|v| v.try_into().whatever_context("convert BitsPerComponent"))
        })
        .whatever_context("get BitsPerComponent")
    }

    fn color_space(&self) -> Result<Option<crate::graphics::ColorSpaceArgs>> {
        self.try_from(&sname("CS"), &sname("ColorSpace"))
            .whatever_context("get ColorSpace")
    }

    fn mask(&self) -> Result<Option<super::ImageMask>> {
        Ok(None)
    }

    fn decode(&self) -> Result<Option<crate::function::Domains>> {
        self.try_from(&sname("D"), &sname("Decode"))
            .whatever_context("get Decode")
    }

    fn image_mask(&self) -> Result<bool> {
        Ok(self
            .alt_get(&sname("IM"), &sname("ImageMask"), Object::bool)
            .whatever_context::<_, ObjectValueError>("get ImageMask")?
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
/// Cache key for inline images to avoid duplicate decoding
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InlineImageCacheKey {
    // Image dimensions
    width: u32,
    height: u32,
    // Optional metadata
    bits_per_component: Option<u8>,
    color_space: String, // Debug string representation
    mask: String,        // Debug string representation
    decode: String,      // Debug string representation
    image_mask: bool,
    // Hash of binary image data
    data_hash: u64,
}

/// Contains image data and metadata of inlined image.
#[derive(Educe)]
#[educe(Default(new))]
pub struct CachedInlineImage(OnceMap<InlineImageCacheKey, Rc<RgbaImage>>);

#[derive(Debug, Clone, PartialEq)]
pub struct InlineImage(Dictionary, Vec<u8>);

impl InlineImage {
    pub fn meta(&self) -> impl ImageMetadata + '_ {
        InlineStreamDict(&self.0)
    }

    fn decode_image_data(
        &self,
        resolver: &ObjectResolver<'_>,
        resources: &ResourceDict<'_, '_>,
    ) -> Result<Rc<RgbaImage>> {
        let decoded_data = decode_inline_stream(&self.0, &self.1, Some(resolver))
            .whatever_context::<_, ObjectValueError>("decode inline image stream")?;

        decode_image(
            decoded_data,
            &InlineStreamDict(&self.0),
            resolver,
            Some(resources),
        )
        .map(|img| Rc::new(img.into_rgba8()))
    }

    pub fn image(
        &self,
        cache: &CachedInlineImage,
        resolver: &ObjectResolver<'_>,
        resources: &ResourceDict<'_, '_>,
    ) -> Result<Rc<RgbaImage>> {
        let meta = self.meta();
        let width = meta.width()?;
        let height = meta.height()?;

        // Skip caching for large images
        if width * height > (150 * 150) {
            return self.decode_image_data(resolver, resources);
        }

        let key = self.get_key()?;
        // Try to get from cache first, if not found decode and insert
        cache
            .0
            .try_insert_cloned(key, |_| self.decode_image_data(resolver, resources))
    }

    /// Generate a cache key for deduplicating identical images
    pub fn get_key(&self) -> Result<InlineImageCacheKey> {
        let meta = self.meta();

        // Calculate MD5 hash of image data
        let mut hasher = ahash::AHasher::default();
        hasher.write(&self.1);
        let data_hash = hasher.finish();

        Ok(InlineImageCacheKey {
            width: meta.width()?,
            height: meta.height()?,
            bits_per_component: meta.bits_per_component()?,
            color_space: format!("{:?}", meta.color_space()?),
            mask: format!("{:?}", meta.mask()?),
            decode: format!("{:?}", meta.decode()?),
            image_mask: meta.image_mask()?,
            data_hash,
        })
    }
}

/// Stub implementation for used in `Operation::PaintInlineImage`,
/// all methods are `unreachable!()`
impl<'b> ConvertFromObject<'b> for InlineImage {
    fn convert_from_object(_objects: &'b mut Vec<Object>) -> Result<Self, ObjectValueError> {
        unreachable!()
    }
}
