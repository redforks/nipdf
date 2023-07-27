use std::{
    borrow::{Borrow, Cow},
    fmt::Display,
    iter::repeat,
    str::from_utf8,
};

use bitstream_io::{BigEndian, BitReader};
use image::{DynamicImage, GrayImage, Luma, RgbImage};
use log::error;
use once_cell::unsync::Lazy;
use pdf2docx_macro::pdf_object;

use crate::{
    ccitt::Flags,
    file::ObjectResolver,
    parser::{ws_prefixed, ParseResult},
};

use super::{Dictionary, Name, Object, ObjectValueError, SchemaDict};

const KEY_FILTER: &[u8] = b"Filter";
const KEY_FILTER_PARAMS: &[u8] = b"DecodeParms";
const KEY_FFILTER: &[u8] = b"FFilter";

const FILTER_FLATE_DECODE: &str = "FlateDecode";
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

fn decode_flate(buf: &[u8], params: Option<&Dictionary>) -> Result<Vec<u8>, ObjectValueError> {
    assert!(params.is_none(), "TODO: handle params of FlateDecode");

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
    image_to_raw: bool,
) -> Result<FilterDecodedData<'a>, ObjectValueError> {
    assert!(
        params.is_none(),
        "TODO: handle params of {}",
        FILTER_DCT_DECODE
    );

    if image_to_raw {
        return Ok(FilterDecodedData::RawImage(RawImage {
            format: ImageFormat::Jpeg,
            data: buf,
        }));
    }

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
    image_to_raw: bool,
) -> Result<FilterDecodedData<'a>, ObjectValueError> {
    assert!(
        params.is_none(),
        "TODO: handle params of {}",
        FILTER_JPX_DECODE
    );

    if image_to_raw {
        return Ok(FilterDecodedData::RawImage(RawImage {
            format: ImageFormat::Jpeg2k,
            data: buf,
        }));
    }

    use jpeg2k::Image;
    let img = handle_filter_error(Image::from_bytes(buf.borrow()), FILTER_JPX_DECODE)?;
    let img = handle_filter_error((&img).try_into(), FILTER_JPX_DECODE)?;
    Ok(FilterDecodedData::Image(img))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, strum::EnumString)]
enum ColorSpace {
    DeviceGray,
    DeviceRGB,
    DeviceCMYK,
    CalGray,
}

#[pdf_object((Some("XObject"), "Image"))]
trait ImageDictTrait {
    fn width(&self) -> u32;
    fn height(&self) -> u32;
    fn bits_per_component(&self) -> Option<u8>;
    #[from_name_str]
    fn color_space(&self) -> Option<ColorSpace>;
}

struct CCITTFaxDecodeParams<'a: 'b, 'b>(&'b Dictionary<'a>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CCITTFGroup {
    #[allow(dead_code)]
    Group3_1D,
    #[allow(dead_code)]
    Group3_2D(i32),
    Group4,
}

impl<'a: 'b, 'b> From<&CCITTFaxDecodeParams<'a, 'b>> for Flags {
    fn from(params: &CCITTFaxDecodeParams<'a, 'b>) -> Self {
        assert!(!params.end_of_line());
        assert!(params.end_of_block());
        assert_eq!(0, params.damaged_rows_before_error());

        Flags {
            encoded_byte_align: params.encoded_byte_align(),
            inverse_black_white: params.black_is1(),
        }
    }
}

pub enum FilterDecodedData<'a> {
    Bytes(Cow<'a, [u8]>),
    Image(DynamicImage),
    RawImage(RawImage<'a>),
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

    /// Convert to bytes, for Image and RawImage returns image bytes.
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Bytes(bytes) => bytes,
            Self::Image(img) => img.as_bytes(),
            Self::RawImage(img) => &img.data[..],
        }
    }
}

impl<'a: 'b, 'b> CCITTFaxDecodeParams<'a, 'b> {
    pub fn k(&self) -> CCITTFGroup {
        match self.0.get_int("K", 0).unwrap() {
            0 => CCITTFGroup::Group3_1D,
            k @ 1.. => CCITTFGroup::Group3_2D(k),
            ..=-1 => CCITTFGroup::Group4,
        }
    }

    pub fn end_of_line(&self) -> bool {
        self.0.get_bool("EndOfLine", false).unwrap()
    }

    pub fn encoded_byte_align(&self) -> bool {
        self.0.get_bool("EncodedByteAlign", false).unwrap()
    }

    pub fn columns(&self) -> u16 {
        self.0.get_int("Columns", 1728).unwrap() as u16
    }

    pub fn rows(&self) -> u16 {
        self.0.get_int("Rows", 0).unwrap() as u16
    }

    pub fn end_of_block(&self) -> bool {
        self.0.get_bool("EndOfBlock", true).unwrap()
    }

    pub fn black_is1(&self) -> bool {
        self.0.get_bool("BlackIs1", false).unwrap()
    }

    pub fn damaged_rows_before_error(&self) -> i32 {
        self.0.get_int("DamagedRowsBeforeError", 0).unwrap()
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
    params: Option<&'b Dictionary<'a>>,
) -> Result<Vec<u8>, ObjectValueError> {
    {
        let params = params;
        use crate::ccitt::decode;

        let empty_params = Lazy::new(Dictionary::new);
        let params = CCITTFaxDecodeParams(params.unwrap_or_else(|| Lazy::force(&empty_params)));
        assert_eq!(params.k(), CCITTFGroup::Group4);
        let image = handle_filter_error(
            decode(
                input,
                params.columns(),
                Some(params.rows() as usize),
                (&params).into(),
            ),
            FILTER_CCITT_FAX,
        )?;
        Ok((image, (params.columns() as u32, params.rows() as u32)))
    }
    .map(|(buf, _meta)| buf)
}

fn filter<'a: 'b, 'b>(
    buf: Cow<'a, [u8]>,
    filter_name: &str,
    params: Option<&'b Dictionary<'a>>,
    image_to_raw: bool,
) -> Result<FilterDecodedData<'a>, ObjectValueError> {
    match filter_name {
        FILTER_FLATE_DECODE => decode_flate(&buf, params).map(FilterDecodedData::bytes),
        FILTER_DCT_DECODE => decode_dct(buf, params, image_to_raw),
        FILTER_CCITT_FAX => decode_ccitt(&buf, params).map(FilterDecodedData::bytes),
        FILTER_ASCII85_DECODE => decode_ascii85(&buf, params).map(FilterDecodedData::bytes),
        FILTER_RUN_LENGTH_DECODE => Ok(FilterDecodedData::bytes(decode_run_length(&buf, params))),
        FILTER_JPX_DECODE => decode_jpx(buf, params, image_to_raw),
        _ => {
            error!("Unknown filter: {}", filter_name);
            Err(ObjectValueError::UnknownFilter)
        }
    }
}

pub enum ImageFormat {
    Jpeg,
    Jpeg2k,
    Png,
}

pub struct RawImage<'a> {
    pub format: ImageFormat,
    pub data: Cow<'a, [u8]>,
}

impl<'a> Stream<'a> {
    pub fn new(dict: Dictionary<'a>, data: &'a [u8]) -> Self {
        Self(dict, data)
    }

    /// Decode stream data using filter and parameters in stream dictionary.
    /// `image_to_raw` if the stream is image, convert to RawImage.
    pub fn decode(
        &self,
        resolver: &ObjectResolver<'a>,
        image_to_raw: bool,
    ) -> Result<FilterDecodedData<'a>, ObjectValueError> {
        let len = resolver
            .resolve_container_value(&self.0, "Length")?
            .1
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

        let mut decoded = FilterDecodedData::Bytes(Cow::Borrowed(&self.1[0..len as usize]));
        for (filter_name, params) in self.iter_filter()? {
            decoded = filter(decoded.into_bytes()?, filter_name, params, image_to_raw)?;
        }

        let img_dict = ImageDict::from(&self.0, resolver)?;
        let Some(img_dict) = img_dict else {
            return Ok(decoded);
        };

        let FilterDecodedData::Bytes(data) = decoded else {
            return Ok(decoded);
        };

        if image_to_raw {
            match (
                img_dict.color_space(),
                img_dict.bits_per_component().unwrap(),
            ) {
                (Some(ColorSpace::DeviceGray), 1) => {
                    use png::{BitDepth, ColorType, Encoder};
                    let mut bytes = Vec::new();
                    let mut encoder = Encoder::new(&mut bytes, img_dict.width(), img_dict.height());
                    encoder.set_color(ColorType::Grayscale);
                    encoder.set_depth(BitDepth::One);
                    let mut writer = encoder.write_header().unwrap();
                    writer.write_image_data(data.borrow()).unwrap();
                    drop(writer);
                    Ok(FilterDecodedData::RawImage(RawImage {
                        format: ImageFormat::Png,
                        data: Cow::Owned(bytes),
                    }))
                }
                (Some(ColorSpace::DeviceGray), 8) => {
                    use png::{BitDepth, ColorType, Encoder};
                    let mut bytes = Vec::new();
                    let mut encoder = Encoder::new(&mut bytes, img_dict.width(), img_dict.height());
                    encoder.set_color(ColorType::Grayscale);
                    encoder.set_depth(BitDepth::Eight);
                    let mut writer = encoder.write_header().unwrap();
                    writer.write_image_data(data.borrow()).unwrap();
                    drop(writer);
                    Ok(FilterDecodedData::RawImage(RawImage {
                        format: ImageFormat::Png,
                        data: Cow::Owned(bytes),
                    }))
                }
                (Some(ColorSpace::DeviceRGB), 8) => {
                    use png::{BitDepth, ColorType, Encoder};
                    let mut bytes = Vec::new();
                    let mut encoder = Encoder::new(&mut bytes, img_dict.width(), img_dict.height());
                    encoder.set_color(ColorType::Rgb);
                    encoder.set_depth(BitDepth::Eight);
                    let mut writer = encoder.write_header().unwrap();
                    writer.write_image_data(data.borrow()).unwrap();
                    drop(writer);
                    Ok(FilterDecodedData::RawImage(RawImage {
                        format: ImageFormat::Png,
                        data: Cow::Owned(bytes),
                    }))
                }
                _ => todo!(
                    "unsupported interoperate decoded stream data as raw image: {:?} {}",
                    img_dict.color_space(),
                    img_dict.bits_per_component().unwrap()
                ),
            }
        } else {
            match (
                img_dict.color_space(),
                img_dict.bits_per_component().unwrap(),
            ) {
                (_, 1) => {
                    use bitstream_io::read::BitRead;

                    let mut img = GrayImage::new(img_dict.width(), img_dict.height());
                    let mut r = BitReader::<_, BigEndian>::new(data.borrow() as &[u8]);
                    for y in 0..img_dict.height() {
                        for x in 0..img_dict.width() {
                            img.put_pixel(
                                x,
                                y,
                                Luma([if r.read_bit().unwrap() { 255u8 } else { 0 }]),
                            );
                        }
                    }
                    Ok(FilterDecodedData::Image(DynamicImage::ImageLuma8(img)))
                }
                (Some(ColorSpace::DeviceGray), 8) => {
                    let img =
                        GrayImage::from_raw(img_dict.width(), img_dict.height(), data.into_owned())
                            .unwrap();
                    Ok(FilterDecodedData::Image(DynamicImage::ImageLuma8(img)))
                }
                (Some(ColorSpace::DeviceRGB), 8) => {
                    let img =
                        RgbImage::from_raw(img_dict.width(), img_dict.height(), data.into_owned())
                            .unwrap();
                    Ok(FilterDecodedData::Image(DynamicImage::ImageRgb8(img)))
                }
                _ => todo!(
                    "unsupported interoperate decoded stream data as image: {:?} {}",
                    img_dict.color_space(),
                    img_dict.bits_per_component().unwrap()
                ),
            }
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
