use std::{
    borrow::{Borrow, Cow},
    fmt::Display,
    iter::{once, repeat},
    str::from_utf8,
};

use anyhow::Result as AnyResult;
use bitstream_io::{BigEndian, BitReader};
use image::{DynamicImage, GrayImage, Luma, Rgb, RgbImage};
use lazy_static::__Deref;
use log::error;
use nipdf_macro::pdf_object;
use once_cell::unsync::Lazy;

use crate::graphics::{ColorSpaceArgs, PredefinedColorSpace};
use crate::{
    ccitt::Flags,
    file::ObjectResolver,
    function::{Function, FunctionDict, Type as FunctionType},
    graphics::{cmyk_to_rgb8, ColorSpace},
    object::PdfObject,
};

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

struct LZWDeflateDecodeParams {
    predictor: i32,
    colors: i32,
    bits_per_comonent: i32,
    columns: i32,
    early_change: i32,
}

impl LZWDeflateDecodeParams {
    pub fn new<'a>(
        d: &Dictionary<'a>,
        r: Option<&ObjectResolver<'a>>,
    ) -> Result<Self, ObjectValueError> {
        Ok(if let Some(r) = r {
            Self {
                predictor: r
                    .opt_resolve_container_value(d, "Predictor")?
                    .map_or(1, |o| o.as_int().unwrap()),
                colors: r
                    .opt_resolve_container_value(d, "Colors")?
                    .map_or(1, |o| o.as_int().unwrap()),
                bits_per_comonent: r
                    .opt_resolve_container_value(d, "BitsPerComponent")?
                    .map_or(8, |o| o.as_int().unwrap()),
                columns: r
                    .opt_resolve_container_value(d, "Columns")?
                    .map_or(1, |o| o.as_int().unwrap()),
                early_change: r
                    .opt_resolve_container_value(d, "EarlyChange")?
                    .map_or(1, |o| o.as_int().unwrap()),
            }
        } else {
            Self {
                predictor: d.get(&b"Predictor"[..]).map_or(1, |o| o.as_int().unwrap()),
                colors: d.get(&b"Colors"[..]).map_or(1, |o| o.as_int().unwrap()),
                bits_per_comonent: d
                    .get(&b"BitsPerComponent"[..])
                    .map_or(8, |o| o.as_int().unwrap()),
                columns: d.get(&b"Columns"[..]).map_or(1, |o| o.as_int().unwrap()),
                early_change: d
                    .get(&b"EarlyChange"[..])
                    .map_or(1, |o| o.as_int().unwrap()),
            }
        })
    }
}

#[pdf_object(())]
trait LZWFlateDecodeDictTrait {
    #[default(1i32)]
    fn predictor(&self) -> i32;

    #[default(1i32)]
    fn colors(&self) -> i32;

    #[default(8i32)]
    fn bits_per_component(&self) -> i32;

    #[default(1i32)]
    fn columns(&self) -> i32;

    #[default(1i32)]
    fn early_change(&self) -> i32;
}

/// Paeth, returns a, b, or c, whichever is closet to a + b - c
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let aa = i16::from(a);
    let bb = i16::from(b);
    let cc = i16::from(c);

    let p = aa + bb - cc;

    let da = (p - aa).abs();
    let db = (p - bb).abs();
    let dc = (p - cc).abs();

    if da <= db && da <= dc {
        a
    } else if db <= dc {
        b
    } else {
        c
    }
}

/// Restore data processed by png predictor.
fn png_predictor(buf: &[u8], columns: i32) -> Result<Vec<u8>, ObjectValueError> {
    let columns = columns as usize;
    let first_row = vec![0u8; columns];
    let mut upper_row = &first_row[..];
    let mut r = vec![0u8; buf.len() / (columns + 1) * columns];
    for (cur_row, dest_row) in buf.chunks(columns + 1).zip(r.chunks_mut(columns)) {
        let (flag, cur_row) = cur_row.split_first().unwrap();
        match flag {
            0 => dest_row.copy_from_slice(cur_row),
            1 => {
                // left
                dest_row[0] = cur_row[0];
                for i in 1..dest_row.len() {
                    dest_row[i] = cur_row[i].wrapping_add(cur_row[i - 1]);
                }
            }
            2 => {
                // up
                for (dest, (up, cur)) in dest_row.iter_mut().zip(upper_row.iter().zip(cur_row)) {
                    *dest = cur.wrapping_add(*up);
                }
            }
            3 => {
                // average of left and up
                let left_row = once(0).chain(cur_row.iter().copied());
                for (dest, (up, (left, cur))) in dest_row
                    .iter_mut()
                    .zip(upper_row.iter().zip(left_row.zip(cur_row.iter())))
                {
                    *dest = (*cur).wrapping_add(((left as i16 + *up as i16) / 2) as u8);
                }
            }
            4 => {
                // paeth
                let left_row = once(0).chain(cur_row.iter().copied());
                let left_upper_row = once(0).chain(upper_row.iter().copied());
                for (dest, (up, (left, (left_up, cur)))) in dest_row.iter_mut().zip(
                    upper_row
                        .iter()
                        .zip(left_row.zip(left_upper_row.zip(cur_row.iter()))),
                ) {
                    *dest = (*cur).wrapping_add(paeth(left, *up, left_up));
                }
            }
            _ => {
                error!("Unknown png predictor: {}", flag);
                return Err(ObjectValueError::FilterDecodeError);
            }
        }
        upper_row = dest_row;
    }
    Ok(r)
}

fn predictor_decode(
    buf: Vec<u8>,
    params: &LZWDeflateDecodeParams,
) -> Result<Vec<u8>, ObjectValueError> {
    match params.predictor {
        1 => Ok(buf),
        10..=15 => png_predictor(
            &buf,
            params.columns * params.bits_per_comonent / 8 * params.colors,
        ),
        2 => todo!("predictor 2/tiff"),
        _ => {
            error!("Unknown predictor: {}", params.predictor);
            Err(ObjectValueError::FilterDecodeError)
        }
    }
}

fn decode_lzw(buf: &[u8], params: LZWDeflateDecodeParams) -> Result<Vec<u8>, ObjectValueError> {
    use weezl::{decode::Decoder, BitOrder};
    let is_early_change = params.early_change == 1;
    let mut decoder = if is_early_change {
        Decoder::with_tiff_size_switch(BitOrder::Msb, 8)
    } else {
        Decoder::new(BitOrder::Msb, 8)
    };
    let mut r = Vec::with_capacity(buf.len() * 2);
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
    predictor_decode(r, &params)
}

fn decode_flate(buf: &[u8], params: LZWDeflateDecodeParams) -> Result<Vec<u8>, ObjectValueError> {
    use flate2::bufread::{DeflateDecoder, ZlibDecoder};
    use std::io::Read;

    let mut r = Vec::with_capacity(buf.len() * 2);
    let mut decoder = ZlibDecoder::new(buf);
    handle_filter_error(
        decoder
            .read_to_end(&mut r)
            .or_else(|_| DeflateDecoder::new(buf).read_to_end(&mut r)),
        FILTER_FLATE_DECODE,
    )?;

    predictor_decode(r, &params)
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
    fn color_space(&self) -> Option<ColorSpaceArgs>;
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
    resolver: Option<&ObjectResolver<'a>>,
    filter_name: &str,
    params: Option<&'b Dictionary<'a>>,
) -> Result<FilterDecodedData<'a>, ObjectValueError> {
    let empty_dict = Lazy::new(Dictionary::new);
    match filter_name {
        FILTER_FLATE_DECODE => decode_flate(
            &buf,
            LZWDeflateDecodeParams::new(params.unwrap_or_else(|| empty_dict.deref()), resolver)?,
        )
        .map(FilterDecodedData::bytes),
        FILTER_DCT_DECODE => decode_dct(buf, params),
        FILTER_CCITT_FAX => decode_ccitt(
            &buf,
            CCITTFaxDecodeParamsDict::new(
                None,
                params.unwrap_or_else(|| empty_dict.deref()),
                resolver.unwrap(),
            )?,
        )
        .map(FilterDecodedData::bytes),
        FILTER_ASCII85_DECODE => decode_ascii85(&buf, params).map(FilterDecodedData::bytes),
        FILTER_RUN_LENGTH_DECODE => Ok(FilterDecodedData::bytes(decode_run_length(&buf, params))),
        FILTER_JPX_DECODE => decode_jpx(buf, params),
        FILTER_LZW_DECODE => decode_lzw(
            &buf,
            LZWDeflateDecodeParams::new(params.unwrap_or_else(|| empty_dict.deref()), resolver)?,
        )
        .map(FilterDecodedData::bytes),
        _ => {
            error!("Unknown filter: {}", filter_name);
            Err(ObjectValueError::UnknownFilter)
        }
    }
}

fn image_transform_color_space(
    resolver: &ObjectResolver,
    img: DynamicImage,
    to: &ColorSpaceArgs,
) -> AnyResult<DynamicImage> {
    fn image_color_space(img: &DynamicImage) -> ColorSpaceArgs {
        match img {
            DynamicImage::ImageLuma8(_) => {
                ColorSpaceArgs::Predefined(PredefinedColorSpace::DeviceGray)
            }
            DynamicImage::ImageRgb8(_) => {
                ColorSpaceArgs::Predefined(PredefinedColorSpace::DeviceRGB)
            }
            _ => todo!("unsupported image color space: {:?}", img),
        }
    }

    fn transform(
        resolver: &ObjectResolver,
        img: DynamicImage,
        from: &ColorSpaceArgs,
        to: &ColorSpaceArgs,
    ) -> AnyResult<DynamicImage> {
        fn gray_by_cymk(img: GrayImage, f: impl Function) -> AnyResult<RgbImage> {
            let mut r = RgbImage::new(img.width(), img.height());
            for (p, dest_p) in img.pixels().zip(r.pixels_mut()) {
                let cymk = f.call(&[p[0] as f32 / 255.0])?;
                let (r, g, b) = cmyk_to_rgb8(cymk[0], cymk[1], cymk[2], cymk[3]);
                *dest_p = Rgb([r, g, b]);
            }
            Ok(r)
        }

        if let (
            ColorSpaceArgs::Predefined(PredefinedColorSpace::DeviceGray),
            ColorSpaceArgs::Separation((alt, id)),
        ) = (from, to)
        {
            if alt.as_ref() == &ColorSpaceArgs::Predefined(PredefinedColorSpace::DeviceCMYK) {
                let f: FunctionDict = resolver.resolve_pdf_object(*id)?;
                return Ok(DynamicImage::ImageRgb8(match f.function_type()? {
                    FunctionType::Sampled => gray_by_cymk(img.into_luma8(), f.sampled()?.func()?)?,
                    _ => todo!(),
                }));
            }
        }
        todo!("transform image color space from {:?} to {:?}", from, to);
    }

    let from = image_color_space(&img);
    if &from == to {
        return Ok(img);
    }

    transform(resolver, img, &from, to)
}

impl<'a> Stream<'a> {
    pub fn new(dict: Dictionary<'a>, data: &'a [u8]) -> Self {
        Self(dict, data)
    }

    pub fn as_dict(&self) -> &Dictionary<'a> {
        &self.0
    }

    pub fn take_dict(self) -> Dictionary<'a> {
        self.0
    }

    /// Get stream un-decoded raw data.
    pub fn raw(&self, resolver: &ObjectResolver<'a>) -> Result<&'a [u8], ObjectValueError> {
        let len = resolver
            .resolve_container_value(&self.0, "Length")?
            .as_int()?;
        Ok(&self.1[0..len as usize])
    }

    fn _decode(
        &self,
        resolver: &ObjectResolver<'a>,
    ) -> Result<FilterDecodedData<'a>, ObjectValueError> {
        let mut decoded = FilterDecodedData::Bytes(self.raw(resolver)?.into());
        for (filter_name, params) in self.iter_filter()? {
            decoded = filter(decoded.into_bytes()?, Some(resolver), filter_name, params)?;
        }
        Ok(decoded)
    }

    /// Decode stream data using filter and parameters in stream dictionary.
    /// `image_to_raw` if the stream is image, convert to RawImage.
    pub fn decode(&self, resolver: &ObjectResolver<'a>) -> Result<Cow<'a, [u8]>, ObjectValueError> {
        self._decode(resolver).and_then(|v| v.into_bytes())
    }

    /// Decode stream but requires that `Length` field is no ref-id, so no need to use `ObjectResolver`
    pub fn decode_without_resolve_length(&self) -> Result<Cow<'a, [u8]>, ObjectValueError> {
        let mut decoded = FilterDecodedData::Bytes(
            self.1[0..self.0.get(&b"Length"[..]).unwrap().as_int().unwrap() as usize].into(),
        );
        for (filter_name, params) in self.iter_filter()? {
            decoded = filter(decoded.into_bytes()?, None, filter_name, params)?;
        }
        decoded.into_bytes()
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
                match (
                    &color_space,
                    img_dict.bits_per_component().unwrap().unwrap(),
                ) {
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
                    (Some(ColorSpaceArgs::Predefined(PredefinedColorSpace::DeviceGray)), 8) => {
                        let img = GrayImage::from_raw(
                            img_dict.width().unwrap(),
                            img_dict.height().unwrap(),
                            data.into_owned(),
                        )
                        .unwrap();
                        DynamicImage::ImageLuma8(img)
                    }
                    (Some(ColorSpaceArgs::Predefined(PredefinedColorSpace::DeviceRGB)), 8) => {
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

        if let Some(args) = color_space.as_ref() {
            Ok(image_transform_color_space(resolver, r, args).unwrap())
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
