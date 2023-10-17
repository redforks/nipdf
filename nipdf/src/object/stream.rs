use std::{
    borrow::{Borrow, Cow},
    fmt::Display,
    iter::repeat,
    str::from_utf8,
};

use bitstream_io::{BigEndian, BitReader};
use image::{DynamicImage, GenericImage, GrayImage, Luma, Pixel, RgbImage};
use lazy_static::__Deref;
use log::error;
use nipdf_macro::pdf_object;
use once_cell::unsync::Lazy;

#[cfg(debug_assertions)]
use crate::parser::{ws_prefixed, ParseResult};
use crate::{ccitt::Flags, file::ObjectResolver, graphics::ColorSpace, object::PdfObject};

use super::{Dictionary, Name, Object, ObjectValueError};

const KEY_FILTER: &[u8] = b"Filter";
const KEY_FILTER_PARAMS: &[u8] = b"DecodeParms";
const KEY_FFILTER: &[u8] = b"FFilter";

const FILTER_FLATE_DECODE: &str = "FlateDecode";
const FILTER_LZW_DECODE: &str = "LZWDecode";
const FILTER_CCITT_FAX: &str = "CCITTFaxDecode";
const FILTER_DCT_DECODE: &str = "DCTDecode";
const FILTER_ASCII85_DECODE: &str = "ASCII85Decode";
const FILTER_RUN_LENGTH_DECODE: &str = "RunLengthDecode";
const FILTER_JPX_DECODE: &str = "JPXDecode";

#[cfg(test)]
const B_FILTER_FLATE_DECODE: &[u8] = FILTER_FLATE_DECODE.as_bytes();
#[cfg(test)]
#[allow(unused)]
const B_FILTER_CCITT_FAX: &[u8] = FILTER_CCITT_FAX.as_bytes();
#[cfg(test)]
#[allow(unused)]
const B_FILTER_DCT_DECODE: &[u8] = FILTER_DCT_DECODE.as_bytes();
#[cfg(test)]
#[allow(unused)]
const B_FILTER_ASCII85_DECODE: &[u8] = FILTER_ASCII85_DECODE.as_bytes();
#[cfg(test)]
#[allow(unused)]
const B_FILTER_RUN_LENGTH_DECODE: &[u8] = FILTER_RUN_LENGTH_DECODE.as_bytes();
#[cfg(test)]
#[allow(unused)]
const B_FILTER_JPX_DECODE: &[u8] = FILTER_JPX_DECODE.as_bytes();

#[derive(Clone, PartialEq, Debug)]
pub struct Stream<'a>(Dictionary<'a>, &'a [u8]); // NOTE: buf end at the file end

/// error!() log if r is error, returns `Err<ObjectValueError::FilterDecodeError>`
fn handle_filter_error<V, E: Display>(
    r: Result<V, E>,
    filter_name: &str,
) -> Result<V, ObjectValueError> {
    r.map_err(|err| {
        error!("Failed to decode stream using {}: {}", filter_name, &err);
        ObjectValueError::FilterDecodeError
    })
}

#[pdf_object(())]
trait LZWFlateDecodeDictTrait {
    #[default(1i32)]
    fn predictor(&self) -> i32;
    #[default(1i32)]
    fn early_change(&self) -> i32;
}

fn decode_lzw(buf: &[u8], params: LZWFlateDecodeDict) -> Result<Vec<u8>, ObjectValueError> {
    assert!(
        params.predictor().unwrap() == 1,
        "TODO: handle predictor of LZWDecode"
    );

    use weezl::{decode::Decoder, BitOrder};
    let is_early_change = params.early_change().unwrap() == 1;
    let mut decoder = if is_early_change {
        Decoder::with_tiff_size_switch(BitOrder::Msb, 8)
    } else {
        Decoder::new(BitOrder::Msb, 8)
    };
    let mut r = vec![];
    let rv = decoder.into_stream(&mut r).decode_all(buf);
    if let Err(e) = rv.status {
        error!("IO error, {:?}", e);
        return Err(ObjectValueError::FilterDecodeError);
    }
    if rv.bytes_read != buf.len() {
        error!(
            "LZWDecode: expected to read {} bytes, but read {} bytes",
            buf.len(),
            rv.bytes_read
        );
        return Err(ObjectValueError::FilterDecodeError);
    }
    Ok(r)
}

fn decode_flate(buf: &[u8], params: LZWFlateDecodeDict) -> Result<Vec<u8>, ObjectValueError> {
    assert!(
        params.predictor().unwrap() == 1,
        "TODO: handle predictor of FlateDecode"
    );

    use flate2::bufread::{DeflateDecoder, ZlibDecoder};
    use std::io::Read;

    let mut output = Vec::with_capacity(buf.len() * 2);
    let mut decoder = ZlibDecoder::new(buf);
    handle_filter_error(
        decoder
            .read_to_end(&mut output)
            .or_else(|_| DeflateDecoder::new(buf).read_to_end(&mut output)),
        FILTER_FLATE_DECODE,
    )?;

    Ok(output)
}

fn decode_dct<'a>(
    buf: Cow<'a, [u8]>,
    params: Option<&Dictionary>,
) -> Result<FilterDecodedData<'a>, ObjectValueError> {
    assert!(
        params.is_none(),
        "TODO: handle params of {}",
        FILTER_DCT_DECODE
    );

    use image::{load_from_memory_with_format, ImageFormat as ImgImageFormat};
    handle_filter_error(
        load_from_memory_with_format(buf.borrow(), ImgImageFormat::Jpeg),
        FILTER_DCT_DECODE,
    )
    .map(FilterDecodedData::Image)
}

fn decode_jpx<'a>(
    buf: Cow<'a, [u8]>,
    params: Option<&Dictionary>,
) -> Result<FilterDecodedData<'a>, ObjectValueError> {
    assert!(
        params.is_none(),
        "TODO: handle params of {}",
        FILTER_JPX_DECODE
    );

    use jpeg2k::Image;
    let img = handle_filter_error(Image::from_bytes(buf.borrow()), FILTER_JPX_DECODE)?;
    let img = handle_filter_error((&img).try_into(), FILTER_JPX_DECODE)?;
    Ok(FilterDecodedData::Image(img))
}

#[pdf_object((Some("XObject"), "Image"))]
pub(crate) trait ImageDictTrait {
    fn width(&self) -> u32;
    fn height(&self) -> u32;
    fn bits_per_component(&self) -> Option<u8>;
    #[try_from]
    fn color_space(&self) -> Option<ColorSpace>;
}

#[pdf_object(())]
trait CCITTFaxDecodeParamsDictTrait {
    #[try_from]
    fn k(&self) -> CCITTFGroup;
    #[or_default]
    fn end_of_line(&self) -> bool;
    #[or_default]
    fn encoded_byte_align(&self) -> bool;
    #[default(1728u16)]
    fn columns(&self) -> u16;
    #[or_default]
    fn rows(&self) -> u16;
    #[default(true)]
    fn end_of_block(&self) -> bool;
    #[or_default]
    fn black_is1(&self) -> bool;
    #[or_default]
    fn damaged_rows_before_error(&self) -> i32;
}

impl<'a: 'b, 'b> TryFrom<&CCITTFaxDecodeParamsDict<'a, 'b>> for Flags {
    type Error = anyhow::Error;
    fn try_from(params: &CCITTFaxDecodeParamsDict<'a, 'b>) -> Result<Self, Self::Error> {
        assert!(!params.end_of_line()?);
        assert!(params.end_of_block()?);
        assert_eq!(0, params.damaged_rows_before_error()?);

        Ok(Flags {
            encoded_byte_align: params.encoded_byte_align()?,
            inverse_black_white: params.black_is1()?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CCITTFGroup {
    #[allow(dead_code)]
    Group3_1D,
    #[allow(dead_code)]
    Group3_2D(i32),
    Group4,
}

impl<'a, 'b> TryFrom<&'b Object<'a>> for CCITTFGroup {
    type Error = ObjectValueError;

    fn try_from(v: &'b Object<'a>) -> Result<Self, Self::Error> {
        Ok(match v.as_int()? {
            0 => Self::Group3_1D,
            k @ 1.. => Self::Group3_2D(k),
            ..=-1 => Self::Group4,
        })
    }
}

enum FilterDecodedData<'a> {
    Bytes(Cow<'a, [u8]>),
    Image(DynamicImage),
}

impl<'a> FilterDecodedData<'a> {
    fn bytes(bytes: Vec<u8>) -> Self {
        Self::Bytes(Cow::Owned(bytes))
    }

    /// Return [[ObjectValueError::StreamIsNotBytes]] if stream is not [[Self::Bytes]]
    fn into_bytes(self) -> Result<Cow<'a, [u8]>, ObjectValueError> {
        match self {
            Self::Bytes(bytes) => Ok(bytes),
            _ => Err(ObjectValueError::StreamIsNotBytes),
        }
    }
}

fn decode_ascii85(
    buf: &[u8],
    params: Option<&Dictionary<'_>>,
) -> Result<Vec<u8>, ObjectValueError> {
    assert!(params.is_none());
    use crate::ascii85::decode;
    handle_filter_error(decode(buf), FILTER_ASCII85_DECODE)
}

fn decode_run_length(buf: &[u8], params: Option<&Dictionary<'_>>) -> Vec<u8> {
    assert!(params.is_none());
    use crate::run_length::decode;
    decode(buf)
}

fn decode_ccitt<'a: 'b, 'b>(
    input: &[u8],
    params: CCITTFaxDecodeParamsDict,
) -> Result<Vec<u8>, ObjectValueError> {
    use crate::ccitt::decode;

    assert_eq!(params.k().unwrap(), CCITTFGroup::Group4);
    let image = handle_filter_error(
        decode(
            input,
            params.columns().unwrap(),
            Some(params.rows().unwrap() as usize),
            (&params).try_into().unwrap(),
        ),
        FILTER_CCITT_FAX,
    )?;
    Ok(image)
}

fn filter<'a: 'b, 'b>(
    buf: Cow<'a, [u8]>,
    resolver: &ObjectResolver<'a>,
    filter_name: &str,
    params: Option<&'b Dictionary<'a>>,
) -> Result<FilterDecodedData<'a>, ObjectValueError> {
    let empty_dict = Lazy::new(Dictionary::new);
    match filter_name {
        FILTER_FLATE_DECODE => decode_flate(
            &buf,
            LZWFlateDecodeDict::new(None, params.unwrap_or_else(|| empty_dict.deref()), resolver)?,
        )
        .map(FilterDecodedData::bytes),
        FILTER_DCT_DECODE => decode_dct(buf, params),
        FILTER_CCITT_FAX => decode_ccitt(
            &buf,
            CCITTFaxDecodeParamsDict::new(
                None,
                params.unwrap_or_else(|| empty_dict.deref()),
                resolver,
            )?,
        )
        .map(FilterDecodedData::bytes),
        FILTER_ASCII85_DECODE => decode_ascii85(&buf, params).map(FilterDecodedData::bytes),
        FILTER_RUN_LENGTH_DECODE => Ok(FilterDecodedData::bytes(decode_run_length(&buf, params))),
        FILTER_JPX_DECODE => decode_jpx(buf, params),
        FILTER_LZW_DECODE => decode_lzw(
            &buf,
            LZWFlateDecodeDict::new(None, params.unwrap_or_else(|| empty_dict.deref()), resolver)?,
        )
        .map(FilterDecodedData::bytes),
        _ => {
            error!("Unknown filter: {}", filter_name);
            Err(ObjectValueError::UnknownFilter)
        }
    }
}

fn image_transform_color_space(img: DynamicImage, to: ColorSpace) -> DynamicImage {
    fn image_color_space(img: &DynamicImage) -> ColorSpace {
        match img {
            DynamicImage::ImageLuma8(_) => ColorSpace::DeviceGray,
            DynamicImage::ImageRgb8(_) => ColorSpace::DeviceRGB,
            _ => todo!("unsupported image color space: {:?}", img),
        }
    }

    fn transform<IMG, P>(img: IMG, from: ColorSpace, to: ColorSpace) -> IMG
    where
        IMG: GenericImage<Pixel = P>,
        P: Pixel,
    {
        todo!();
    }

    let from = image_color_space(&img);
    if from == to {
        return img;
    }

    transform(img, from, to)
}

impl<'a> Stream<'a> {
    pub fn new(dict: Dictionary<'a>, data: &'a [u8]) -> Self {
        Self(dict, data)
    }

    pub fn as_dict(&self) -> &Dictionary<'a> {
        &self.0
    }

    /// Get stream un-decoded raw data.
    pub fn raw(&self, resolver: &ObjectResolver<'a>) -> Result<&'a [u8], ObjectValueError> {
        let len = resolver
            .resolve_container_value(&self.0, "Length")?
            .as_int()?;
        #[cfg(debug_assertions)]
        {
            let end_stream = &self.1[len as usize..];
            let rv: ParseResult<_> =
                ws_prefixed(nom::bytes::complete::tag(b"endstream"))(end_stream);
            if rv.is_err() {
                panic!("{:#?}", self.1);
            }
        }
        Ok(&self.1[0..len as usize])
    }

    fn _decode(
        &self,
        resolver: &ObjectResolver<'a>,
    ) -> Result<FilterDecodedData<'a>, ObjectValueError> {
        let mut decoded = FilterDecodedData::Bytes(self.raw(resolver)?.into());
        for (filter_name, params) in self.iter_filter()? {
            decoded = filter(decoded.into_bytes()?, resolver, filter_name, params)?;
        }
        Ok(decoded)
    }

    /// Decode stream data using filter and parameters in stream dictionary.
    /// `image_to_raw` if the stream is image, convert to RawImage.
    pub fn decode(&self, resolver: &ObjectResolver<'a>) -> Result<Cow<'a, [u8]>, ObjectValueError> {
        self._decode(resolver).and_then(|v| v.into_bytes())
    }

    pub fn decode_image(
        &self,
        resolver: &ObjectResolver<'a>,
    ) -> Result<DynamicImage, ObjectValueError> {
        let decoded = self._decode(resolver)?;
        let img_dict = ImageDict::new(None, &self.0, resolver)?;

        let color_space = img_dict.color_space().unwrap();
        let r = match decoded {
            FilterDecodedData::Image(img) => img,
            FilterDecodedData::Bytes(data) => {
                match (color_space, img_dict.bits_per_component().unwrap().unwrap()) {
                    (_, 1) => {
                        use bitstream_io::read::BitRead;

                        let mut img =
                            GrayImage::new(img_dict.width().unwrap(), img_dict.height().unwrap());
                        let mut r = BitReader::<_, BigEndian>::new(data.borrow() as &[u8]);
                        for y in 0..img_dict.height().unwrap() {
                            for x in 0..img_dict.width().unwrap() {
                                img.put_pixel(
                                    x,
                                    y,
                                    Luma([if r.read_bit().unwrap() { 255u8 } else { 0 }]),
                                );
                            }
                        }
                        DynamicImage::ImageLuma8(img)
                    }
                    (Some(ColorSpace::DeviceGray), 8) => {
                        let img = GrayImage::from_raw(
                            img_dict.width().unwrap(),
                            img_dict.height().unwrap(),
                            data.into_owned(),
                        )
                        .unwrap();
                        DynamicImage::ImageLuma8(img)
                    }
                    (Some(ColorSpace::DeviceRGB), 8) => {
                        let img = RgbImage::from_raw(
                            img_dict.width().unwrap(),
                            img_dict.height().unwrap(),
                            data.into_owned(),
                        )
                        .unwrap();
                        DynamicImage::ImageRgb8(img)
                    }
                    _ => todo!(
                        "unsupported interoperate decoded stream data as image: {:?} {}",
                        img_dict.color_space(),
                        img_dict.bits_per_component().unwrap().unwrap()
                    ),
                }
            }
        };

        if let Some(color_space) = color_space {
            Ok(image_transform_color_space(r, color_space))
        } else {
            Ok(r)
        }
    }

    fn iter_filter(
        &self,
    ) -> Result<impl Iterator<Item = (&str, Option<&Dictionary<'a>>)>, ObjectValueError> {
        if self.0.contains_key(&Name::borrowed(KEY_FFILTER)) {
            return Err(ObjectValueError::ExternalStreamNotSupported);
        }

        let filters = self.0.get(&Name::borrowed(KEY_FILTER)).map_or_else(
            || Ok(vec![]),
            |v| match v {
                Object::Array(vals) => vals
                    .iter()
                    .map(|v| v.as_name().map_err(|_| ObjectValueError::UnexpectedType))
                    .collect(),
                Object::Name(n) => Ok(vec![from_utf8(n.0.borrow()).unwrap()]),
                _ => Err(ObjectValueError::UnexpectedType),
            },
        )?;
        let params = self.0.get(&Name::borrowed(KEY_FILTER_PARAMS)).map_or_else(
            || Ok(vec![]),
            |v| match v {
                Object::Null => Ok(vec![]),
                Object::Array(vals) => vals
                    .iter()
                    .map(|v| match v {
                        Object::Null => Ok(None),
                        Object::Dictionary(dict) => Ok(Some(dict)),
                        _ => Err(ObjectValueError::UnexpectedType),
                    })
                    .collect(),
                Object::Dictionary(dict) => Ok(vec![Some(dict)]),
                _ => Err(ObjectValueError::UnexpectedType),
            },
        )?;
        Ok(filters
            .into_iter()
            .zip(params.into_iter().chain(repeat(None))))
    }
}

#[cfg(test)]
mod tests;
