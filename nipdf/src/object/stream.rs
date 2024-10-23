use super::{Dictionary, Object, ObjectId, ObjectValueError};
use crate::{
    ccitt::{Algorithm as CCITTAlgorithm, Flags},
    file::{EncryptInfo, ObjectResolver, ResourceDict},
    function::Domains,
    graphics::{
        ColorSpaceArgs,
        color_space::{
            ColorCompConvertTo, ColorSpace, ColorSpaceTrait, DeviceCMYK, color_to_rgba,
            convert_color_to,
        },
    },
    object::PdfObject,
    parser::is_white_space,
};
use anyhow::Result as AnyResult;
use bitstream_io::{BigEndian, BitReader};
use image::{DynamicImage, GrayImage, Luma, RgbImage, Rgba, RgbaImage};
use jpeg_decoder::PixelFormat;
use log::error;
use nipdf_macro::pdf_object;
use num_traits::ToPrimitive;
use once_cell::unsync::Lazy;
use prescript::{Name, sname};
use std::{
    borrow::{Borrow, Cow},
    fmt::Display,
    iter::{once, repeat},
    num::NonZeroU32,
    ops::Range,
    rc::Rc,
};
use tinyvec::TinyVec;

const KEY_FILTER: Name = sname("Filter");
const KEY_FILTER_PARAMS: Name = sname("DecodeParms");
const KEY_FFILTER: Name = sname("FFilter");
mod inline_image;
pub use inline_image::*;

const S_FILTER_CRYPT: &str = "Crypt";
const S_FILTER_FLATE_DECODE: &str = "FlateDecode";
const S_FILTER_LZW_DECODE: &str = "LZWDecode";
const S_FILTER_CCITT_FAX: &str = "CCITTFaxDecode";
const S_FILTER_DCT_DECODE: &str = "DCTDecode";
const S_FILTER_ASCII85_DECODE: &str = "ASCII85Decode";
const S_FILTER_ASCII_HEX_DECODE: &str = "ASCIIHexDecode";
const S_FILTER_RUN_LENGTH_DECODE: &str = "RunLengthDecode";
const S_FILTER_JPX_DECODE: &str = "JPXDecode";

const FILTER_CRYPT: Name = sname(S_FILTER_CRYPT);
#[cfg(test)]
const FILTER_FLATE_DECODE: Name = sname("FlateDecode");
// const FILTER_LZW_DECODE: Name = sname("LZWDecode");
const FILTER_CCITT_FAX: Name = sname("CCITTFaxDecode");
const FILTER_DCT_DECODE: Name = sname("DCTDecode");
const FILTER_ASCII85_DECODE: Name = sname("ASCII85Decode");
// const FILTER_RUN_LENGTH_DECODE: Name = sname("RunLengthDecode");
const FILTER_JPX_DECODE: Name = sname("JPXDecode");

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct BufPos {
    start: u32,
    length: Option<NonZeroU32>,
}

impl BufPos {
    pub fn new(start: u32, length: Option<NonZeroU32>) -> Self {
        Self { start, length }
    }

    /// Return [start..(start+length)] if length not None, otherwise call
    /// `f` to resolve length
    pub fn range<E>(&self, f: impl FnOnce() -> Result<u32, E>) -> Result<Range<usize>, E> {
        let start = self.start as usize;
        let length = self.length.map_or_else(f, |v| Ok(u32::from(v)))? as usize;
        Ok(start..(start + length))
    }
}

struct FilterDict<'a, 'b> {
    d: &'b Dictionary,
    r: Option<&'b ObjectResolver<'a>>,
}

impl<'a, 'b> FilterDict<'a, 'b> {
    pub fn new(
        d: &'b Dictionary,
        r: Option<&'b ObjectResolver<'a>>,
    ) -> Result<Self, ObjectValueError> {
        if d.contains_key(&KEY_FFILTER) {
            return Err(ObjectValueError::ExternalStreamNotSupported);
        }

        Ok(Self { d, r })
    }

    fn alt_get(&self, id1: &Name, id2: &Name) -> Option<&'b Object> {
        self.d.get(id1).or_else(|| self.d.get(id2))
    }

    fn get_filters(
        v: &Object,
        r: Option<&'b ObjectResolver<'a>>,
    ) -> Result<Vec<Name>, ObjectValueError> {
        Ok(match v {
            Object::Array(vals) => vals
                .iter()
                .map(|v| v.name().map_err(|_| ObjectValueError::UnexpectedType))
                .collect::<Result<_, _>>()?,
            Object::Name(n) => vec![n.clone()],
            Object::Reference(id) => Self::get_filters(r.unwrap().resolve(id.id().id())?, r)?,
            _ => {
                error!("Filter is not Name or Array of Name");
                return Err(ObjectValueError::UnexpectedType);
            }
        })
    }

    /// Get object value of `Filter` field, or `F` field if `Filter` not defined.
    /// If value is array, its items should all be Name,
    /// Otherwise, it should be Name.
    pub fn filters(&self) -> Result<Vec<Name>, ObjectValueError> {
        let v = self.alt_get(&KEY_FILTER, &sname("F"));
        let Some(v) = v else {
            return Ok(vec![]);
        };

        Self::get_filters(v, self.r)
    }

    /// Get object value of `DecodeParms` field, or `DP` field if `DecodeParms` not defined.
    /// If value is array, its items should be Dictionary or None,
    /// Otherwise, it should be Dictionary.
    pub fn parameters(&self) -> Result<Vec<Option<&'b Dictionary>>, ObjectValueError> {
        let v = self.alt_get(&KEY_FILTER_PARAMS, &sname("DP"));
        let Some(v) = v else {
            return Ok(vec![]);
        };

        Ok(match v {
            Object::Array(vals) => vals
                .iter()
                .map(|v| match v {
                    Object::Dictionary(d) => Ok(Some(d)),
                    Object::Null => Ok(None),
                    Object::Reference(r) => self
                        .r
                        .unwrap()
                        .resolve(r.id().id())
                        .and_then(|o| o.as_dict().map(Some)),
                    _ => {
                        error!("DecodeParms is not Dictionary or Array of Dictionary");
                        Err(ObjectValueError::UnexpectedType)
                    }
                })
                .collect::<Result<_, _>>()?,
            Object::Dictionary(d) => vec![Some(d)],
            _ => {
                error!("DecodeParms is not Dictionary or Array of Dictionary");
                return Err(ObjectValueError::UnexpectedType);
            }
        })
    }
}

/// Iterate pairs of filter name and its parameter
fn iter_filters<'b>(
    d: FilterDict<'_, 'b>,
) -> Result<impl Iterator<Item = (Name, Option<&'b Dictionary>)>, ObjectValueError> {
    let filters = d.filters()?;
    let params = d.parameters()?;
    Ok(filters
        .into_iter()
        .zip(params.into_iter().chain(repeat(None))))
}

/// Provides common implementation to decode stream data,
/// to share implementation for `Stream` and `InlineStream`
fn decode_stream<'a, 'b>(
    filter_dict: &'b Dictionary,
    buf: impl Into<Cow<'a, [u8]>>,
    resolver: Option<&ObjectResolver<'a>>,
    encrypt_info: Option<&EncryptInfo>,
    id: Option<ObjectId>,
) -> Result<FilterDecodedData<'a>, ObjectValueError> {
    let encrypt_info = encrypt_info.or_else(|| resolver.and_then(|r| r.encript_info()));
    let filter_dict = FilterDict::new(filter_dict, resolver)?;
    let mut decoded = FilterDecodedData::Bytes(buf.into());
    let filters = iter_filters(filter_dict)?;
    if let Some(encryp_info) = encrypt_info {
        let mut filters = filters.peekable();
        if !filters
            .peek()
            .map_or_else(|| false, |(f, _)| f == &FILTER_CRYPT)
        {
            // pre a Crypt filter if enabled encrypt and Crypt not a first filter
            let filters = once((FILTER_CRYPT, None)).chain(filters);
            for (filter_name, params) in filters {
                decoded = filter(
                    decoded.into_bytes()?,
                    resolver,
                    &filter_name,
                    params,
                    id,
                    Some(encryp_info),
                )?;
            }
        } else {
            for (filter_name, params) in filters {
                decoded = filter(
                    decoded.into_bytes()?,
                    resolver,
                    &filter_name,
                    params,
                    id,
                    Some(encryp_info),
                )?;
            }
        }
    } else {
        for (filter_name, params) in filters {
            decoded = filter(
                decoded.into_bytes()?,
                resolver,
                &filter_name,
                params,
                id,
                None,
            )?;
        }
    }

    Ok(decoded)
}

/// Abstract image metadata.for decode image from `Stream` and `InlineStream`
pub trait ImageMetadata {
    fn width(&self) -> AnyResult<u32>;
    fn height(&self) -> AnyResult<u32>;
    fn bits_per_component(&self) -> AnyResult<Option<u8>>;
    fn color_space(&self) -> AnyResult<Option<ColorSpaceArgs>>;
    fn image_mask(&self) -> AnyResult<bool>;
    fn mask(&self) -> AnyResult<Option<ImageMask>>;
    fn decode(&self) -> AnyResult<Option<Domains>>;
}

fn decode_image<'a, M: ImageMetadata>(
    data: FilterDecodedData<'a>,
    img_meta: &M,
    resolver: &ObjectResolver<'a>,
    resources: Option<&ResourceDict<'a, '_>>,
) -> Result<DynamicImage, ObjectValueError> {
    fn decode_one_bit(w: u32, h: u32, data: &[u8], row_padding: bool) -> DynamicImage {
        use bitstream_io::read::BitRead;

        let mut img = GrayImage::new(w, h);
        let row_padding_bits = if row_padding {
            let remain_bits = w % 8;
            if remain_bits != 0 { 8 - remain_bits } else { 0 }
        } else {
            0
        };

        let mut r = BitReader::<_, BigEndian>::new(data as &[u8]);
        for y in 0..h {
            for x in 0..w {
                img.put_pixel(x, y, Luma([if r.read_bit().unwrap() { 255u8 } else { 0 }]));
            }
            r.skip(row_padding_bits).unwrap();
        }
        DynamicImage::ImageLuma8(img)
    }

    let color_space = img_meta.color_space().unwrap();
    let color_space =
        color_space.map(|args| ColorSpace::from_args(&args, resolver, resources).unwrap());
    let mut r = match data {
        FilterDecodedData::Image(img) => {
            if let Some(color_space) = color_space.as_ref() {
                image_transform_color_space(img, color_space).unwrap()
            } else {
                img
            }
        }

        FilterDecodedData::CmykImage((width, height, pixels)) => {
            let cs = DeviceCMYK;
            DynamicImage::ImageRgba8(RgbaImage::from_fn(width, height, |x, y| {
                let i = (y * width + x) as usize * 4;
                Rgba(cs.to_rgba(&[
                    255 - pixels[i],
                    255 - pixels[i + 1],
                    255 - pixels[i + 2],
                    255 - pixels[i + 3],
                ]))
            }))
        }

        FilterDecodedData::Bytes(data) => {
            match (
                &color_space,
                img_meta.bits_per_component().unwrap().unwrap(),
            ) {
                (_, 1) => decode_one_bit(
                    img_meta.width().unwrap(),
                    img_meta.height().unwrap(),
                    data.borrow(),
                    true,
                ),
                (Some(cs), 8) => {
                    let n_colors = cs.components();
                    let mut img =
                        RgbaImage::new(img_meta.width().unwrap(), img_meta.height().unwrap());
                    for (p, dest_p) in data.chunks(n_colors).zip(img.pixels_mut()) {
                        let c: TinyVec<[f32; 4]> = p.iter().map(|v| v.into_color_comp()).collect();
                        let color: [u8; 4] = color_to_rgba(cs, c.as_slice());
                        *dest_p = Rgba(color);
                    }
                    DynamicImage::ImageRgba8(img)
                }
                _ => todo!(
                    "unsupported interoperate decoded stream data as image: {:?} {}",
                    color_space,
                    img_meta.bits_per_component().unwrap().unwrap()
                ),
            }
        }

        FilterDecodedData::CCITTFaxImage(data) => {
            assert_eq!(1, img_meta.bits_per_component().unwrap().unwrap());
            decode_one_bit(
                img_meta.width().unwrap(),
                img_meta.height().unwrap(),
                &data,
                false,
            )
        }
    };

    if let Some(ImageMask::ColorKey(color_key)) = img_meta.mask().unwrap() {
        let Some(cs) = color_space else {
            todo!("Color Space not defined when process color key mask");
        };
        let mut img = r.into_rgba8();
        let color_key = color_key_range(&color_key, &cs);

        for p in img.pixels_mut() {
            // set alpha color to 0 if its rgb color in color_key range inclusive
            if color_matches_color_key(color_key, p.0) {
                p[3] = 0;
            }
        }
        r = DynamicImage::ImageRgba8(img);
    }

    Ok(r)
}

impl<'a, 'b> ImageMetadata for ImageDict<'a, 'b> {
    fn width(&self) -> AnyResult<u32> {
        self.width()
    }

    fn height(&self) -> AnyResult<u32> {
        self.height()
    }

    fn bits_per_component(&self) -> AnyResult<Option<u8>> {
        self.bits_per_component()
    }

    fn color_space(&self) -> AnyResult<Option<ColorSpaceArgs>> {
        self.color_space()
    }

    fn image_mask(&self) -> AnyResult<bool> {
        self.image_mask()
    }

    fn mask(&self) -> AnyResult<Option<ImageMask>> {
        self.mask()
    }

    fn decode(&self) -> AnyResult<Option<Domains>> {
        self.decode()
    }
}

// 2nd value is offset of stream data from the begin of indirect object
#[derive(Clone, PartialEq, Debug)]
pub struct Stream(
    pub(crate) Dictionary,
    pub(crate) BufPos,
    pub(crate) ObjectId,
);

/// error!() log if r is error, returns `Err<ObjectValueError::FilterDecodeError>`
fn handle_filter_error<V, E: Display>(
    r: Result<V, E>,
    filter_name: &Name,
) -> Result<V, ObjectValueError> {
    r.map_err(|err| {
        error!("Failed to decode stream using {}: {}", filter_name, &err);
        ObjectValueError::FilterDecodeError
    })
}

struct LZWDeflateDecodeParams {
    predictor: i32,
    colors: i32,
    bits_per_component: i32,
    columns: i32,
    early_change: i32,
}

impl LZWDeflateDecodeParams {
    pub fn new(d: &Dictionary, r: Option<&ObjectResolver<'_>>) -> Result<Self, ObjectValueError> {
        Ok(if let Some(r) = r {
            Self {
                predictor: r
                    .opt_resolve_container_value(d, &sname("Predictor"))?
                    .map_or(1, |o| o.int().unwrap()),
                colors: r
                    .opt_resolve_container_value(d, &sname("Colors"))?
                    .map_or(1, |o| o.int().unwrap()),
                bits_per_component: r
                    .opt_resolve_container_value(d, &sname("BitsPerComponent"))?
                    .map_or(8, |o| o.int().unwrap()),
                columns: r
                    .opt_resolve_container_value(d, &sname("Columns"))?
                    .map_or(1, |o| o.int().unwrap()),
                early_change: r
                    .opt_resolve_container_value(d, &sname("EarlyChange"))?
                    .map_or(1, |o| o.int().unwrap()),
            }
        } else {
            Self {
                predictor: d.get(&sname("Predictor")).map_or(1, |o| o.int().unwrap()),
                colors: d.get(&sname("Colors")).map_or(1, |o| o.int().unwrap()),
                bits_per_component: d
                    .get(&sname("BitsPerComponent"))
                    .map_or(8, |o| o.int().unwrap()),
                columns: d.get(&sname("Columns")).map_or(1, |o| o.int().unwrap()),
                early_change: d.get(&sname("EarlyChange")).map_or(1, |o| o.int().unwrap()),
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
fn png_predictor(
    buf: &[u8],
    row_bytes: usize,
    pixel_bytes: usize,
) -> Result<Vec<u8>, ObjectValueError> {
    let row_with_flag_bytes = 1 + row_bytes;
    assert_eq!(buf.len() % row_with_flag_bytes, 0);
    let first_row = vec![0u8; row_bytes];
    let mut upper_row = &first_row[..];
    let mut r = vec![0u8; buf.len() / row_with_flag_bytes * row_bytes];

    for (cur_row, dest_row) in buf.chunks(row_with_flag_bytes).zip(r.chunks_mut(row_bytes)) {
        let (flag, cur_row) = cur_row.split_first().unwrap();
        match flag {
            0 => dest_row.copy_from_slice(cur_row),
            1 => {
                // left
                dest_row[..pixel_bytes].copy_from_slice(&cur_row[..pixel_bytes]);
                for i in pixel_bytes..row_bytes {
                    dest_row[i] = cur_row[i].wrapping_add(dest_row[i - pixel_bytes]);
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
                for i in 0..pixel_bytes {
                    dest_row[i] = cur_row[i].wrapping_add(upper_row[i]);
                }
                #[allow(clippy::cast_possible_truncation)]
                for i in pixel_bytes..row_bytes {
                    dest_row[i] = cur_row[i].wrapping_add(
                        ((dest_row[i - pixel_bytes] as u16 + upper_row[i] as u16) / 2) as u8,
                    );
                }
            }
            4 => {
                // paeth
                for i in 0..pixel_bytes {
                    dest_row[i] = cur_row[i].wrapping_add(paeth(0, upper_row[i], 0));
                }
                for i in pixel_bytes..row_bytes {
                    dest_row[i] = cur_row[i].wrapping_add(paeth(
                        dest_row[i - pixel_bytes],
                        upper_row[i],
                        upper_row[i - pixel_bytes],
                    ));
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
            (params.columns * params.colors * params.bits_per_component + 7) as usize / 8,
            (params.colors * params.bits_per_component + 7) as usize / 8,
        ),
        2 => todo!("predictor 2/tiff"),
        _ => {
            error!("Unknown predictor: {}", params.predictor);
            Err(ObjectValueError::FilterDecodeError)
        }
    }
}

fn decode_lzw(buf: &[u8], params: LZWDeflateDecodeParams) -> Result<Vec<u8>, ObjectValueError> {
    use weezl::{BitOrder, decode::Decoder};
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

fn crypt_filter(
    mut buf: Vec<u8>,
    id: ObjectId,
    encrypt_info: &EncryptInfo,
    params: Option<&Dictionary>,
) -> Result<Vec<u8>, ObjectValueError> {
    let name = params
        .and_then(|d| d.get("Name").map(|o| o.name()))
        .transpose()?;
    encrypt_info.stream_decrypt(name, id, &mut buf);
    Ok(buf)
}

/// inflate zlib/deflate data, auto detect zlib or deflate, ignore adler32 checksum(some pdf file
/// has wrong adler32 checksum).
///
/// Max output buffer size is 128MB, return error if output buffer size exceed this limit.
fn deflate(input: &[u8]) -> Result<Vec<u8>, ObjectValueError> {
    use miniz_oxide::inflate::{
        TINFLStatus,
        core::{DecompressorOxide, decompress, inflate_flags},
    };

    fn _deflate(input: &[u8], flags: u32) -> Result<Vec<u8>, ObjectValueError> {
        const MAX_OUTPUT_SIZE: usize = 128 * 1024 * 1024;

        let flags = flags
            | inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF
            | inflate_flags::TINFL_FLAG_IGNORE_ADLER32;
        let mut ret: Vec<u8> = vec![0; input.len().wrapping_mul(2).min(MAX_OUTPUT_SIZE)];

        let mut decomp = Box::<DecompressorOxide>::default();

        let mut in_pos = 0;
        let mut out_pos = 0;
        loop {
            // Wrap the whole output slice so we know we have enough of the
            // decompressed data for matches.
            let (status, in_consumed, out_consumed) =
                decompress(&mut decomp, &input[in_pos..], &mut ret, out_pos, flags);
            in_pos += in_consumed;
            out_pos += out_consumed;

            match status {
                TINFLStatus::Done => {
                    ret.truncate(out_pos);
                    return Ok(ret);
                }

                TINFLStatus::HasMoreOutput => {
                    // if the buffer has already reached the size limit, return an error
                    if ret.len() >= MAX_OUTPUT_SIZE {
                        error!("inflate: has more output");
                        return Err(ObjectValueError::FilterDecodeError);
                    }
                    // calculate the new length, capped at `max_output_size`
                    let new_len = ret.len().saturating_mul(2).min(MAX_OUTPUT_SIZE);
                    ret.resize(new_len, 0);
                }

                _ => {
                    if status == TINFLStatus::FailedCannotMakeProgress {
                        // ignore truncated zlib data, see deflate_recover_truncated_zlib_data()
                        // unit test
                        error!("inflate: need more data");
                        ret.truncate(out_pos);
                        return Ok(ret);
                    }
                    error!("inflate: error: {:?}", status);
                    return Err(ObjectValueError::FilterDecodeError);
                }
            }
        }
    }

    _deflate(input, inflate_flags::TINFL_FLAG_PARSE_ZLIB_HEADER).or_else(|_| _deflate(input, 0))
}

fn decode_flate(buf: &[u8], params: LZWDeflateDecodeParams) -> Result<Vec<u8>, ObjectValueError> {
    deflate(buf).and_then(|r| predictor_decode(r, &params))
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

    use jpeg_decoder::Decoder;
    let mut decoder = Decoder::new(buf.as_ref());
    let pixels = handle_filter_error(decoder.decode(), &FILTER_DCT_DECODE)?;
    let info = decoder.info().unwrap();

    match info.pixel_format {
        PixelFormat::L8 => Ok(FilterDecodedData::Image(DynamicImage::ImageLuma8(
            GrayImage::from_vec(info.width as u32, info.height as u32, pixels).unwrap(),
        ))),
        PixelFormat::L16 => {
            todo!("Convert to DynamicImage::ImageLuma16")
            // Problem is jpeg-decoder returns pixels in Vec<u8>, but DynamicImage::ImageLuma16
            // expect Vec<u16> I don't known is {little,big}-endian, or native-endian in
            // pixels
        }
        PixelFormat::RGB24 => Ok(FilterDecodedData::Image(DynamicImage::ImageRgb8(
            RgbImage::from_vec(info.width as u32, info.height as u32, pixels).unwrap(),
        ))),
        PixelFormat::CMYK32 => Ok(FilterDecodedData::CmykImage((
            info.width as u32,
            info.height as u32,
            pixels,
        ))),
    }
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
    let img = handle_filter_error(Image::from_bytes(buf.borrow()), &FILTER_JPX_DECODE)?;
    let img = handle_filter_error((&img).try_into(), &FILTER_JPX_DECODE)?;
    Ok(FilterDecodedData::Image(img))
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImageMask {
    Explicit(Rc<Stream>),
    ColorKey(Domains),
}

impl TryFrom<&Object> for ImageMask {
    type Error = ObjectValueError;

    fn try_from(v: &Object) -> Result<Self, Self::Error> {
        Ok(match v {
            Object::Stream(s) => Self::Explicit(s.clone()),
            Object::Array(_) => {
                let domains = Domains::try_from(v)?;
                Self::ColorKey(domains)
            }
            _ => return Err(ObjectValueError::UnexpectedType),
        })
    }
}

#[pdf_object((Some("XObject"), "Image"))]
pub(crate) trait ImageDictTrait {
    fn width(&self) -> u32;
    fn height(&self) -> u32;
    fn bits_per_component(&self) -> Option<u8>;
    #[try_from]
    fn color_space(&self) -> Option<ColorSpaceArgs>;

    #[or_default]
    fn image_mask(&self) -> bool;

    #[try_from]
    fn mask(&self) -> Option<ImageMask>;
    #[try_from]
    fn decode(&self) -> Option<Domains>;
}

#[pdf_object(())]
trait CCITTFaxDecodeParamsDictTrait {
    #[try_from]
    fn k(&self) -> CCITTAlgorithm;
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
        assert_eq!(0, params.damaged_rows_before_error()?);

        Ok(Flags {
            encoded_byte_align: params.encoded_byte_align()?,
            inverse_black_white: params.black_is1()?,
            end_of_block: params.end_of_block()?,
        })
    }
}

impl<'b> TryFrom<&'b Object> for CCITTAlgorithm {
    type Error = ObjectValueError;

    fn try_from(v: &'b Object) -> Result<Self, Self::Error> {
        Ok(match v.int()? {
            0 => Self::Group3_1D,
            k @ 1.. => Self::Group3_2D(k.try_into().unwrap()),
            ..=-1 => Self::Group4,
        })
    }
}

enum FilterDecodedData<'a> {
    Bytes(Cow<'a, [u8]>),
    Image(DynamicImage),
    CCITTFaxImage(Vec<u8>),         // width, height, data
    CmykImage((u32, u32, Vec<u8>)), // width, height, data
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

/// decode ASCIIHexDecode encoded stream data.
/// Ignore whitespace bytes.
/// '>' means end of stream, assume '0' if last hex digit is missing.
fn decode_ascii_hex(buf: &[u8]) -> Result<Vec<u8>, ObjectValueError> {
    let mut r = Vec::with_capacity(buf.len() / 2);
    let mut iter = buf.iter().filter(|&&b| !is_white_space(b));
    while let Some(&b) = iter.next() {
        let b = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            b'>' => break,
            _ => {
                error!("Invalid ASCIIHexDecode: {}", b);
                return Err(ObjectValueError::FilterDecodeError);
            }
        };
        let b = b << 4
            | match iter.next() {
                Some(&b) => match b {
                    b'0'..=b'9' => b - b'0',
                    b'a'..=b'f' => b - b'a' + 10,
                    b'A'..=b'F' => b - b'A' + 10,
                    b'>' => 0,
                    _ => {
                        error!("Invalid ASCIIHexDecode: {}", b);
                        return Err(ObjectValueError::FilterDecodeError);
                    }
                },
                None => 0,
            };
        r.push(b);
    }
    Ok(r)
}

fn decode_ascii85(buf: &[u8], params: Option<&Dictionary>) -> Result<Vec<u8>, ObjectValueError> {
    assert!(params.is_none());
    use crate::ascii85::decode;
    handle_filter_error(decode(buf), &FILTER_ASCII85_DECODE)
}

fn decode_run_length(buf: &[u8], params: Option<&Dictionary>) -> Vec<u8> {
    assert!(params.is_none());
    use crate::run_length::decode;
    decode(buf)
}

fn decode_ccitt<'a: 'b, 'b>(
    input: &[u8],
    params: CCITTFaxDecodeParamsDict,
) -> Result<Vec<u8>, ObjectValueError> {
    use crate::ccitt::Decoder;

    let decoder = Decoder {
        algorithm: params.k().unwrap(),
        width: params.columns().unwrap(),
        rows: Some(params.rows().unwrap()),
        flags: (&params).try_into().unwrap(),
    };
    let image = handle_filter_error(decoder.decode(input), &FILTER_CCITT_FAX)?;
    Ok(image)
}

fn filter<'a: 'b, 'b>(
    buf: Cow<'a, [u8]>,
    resolver: Option<&ObjectResolver<'a>>,
    filter_name: &Name,
    params: Option<&'b Dictionary>,
    id: Option<ObjectId>,
    encrypt_info: Option<&EncryptInfo>,
) -> Result<FilterDecodedData<'a>, ObjectValueError> {
    let empty_dict = Lazy::new(Dictionary::new);
    #[allow(clippy::match_ref_pats)]
    match filter_name.as_str() {
        S_FILTER_CRYPT => {
            crypt_filter(buf.into_owned(), id.unwrap(), encrypt_info.unwrap(), params)
                .map(FilterDecodedData::bytes)
        }
        S_FILTER_FLATE_DECODE => decode_flate(
            &buf,
            LZWDeflateDecodeParams::new(params.unwrap_or_else(|| &*empty_dict), resolver)?,
        )
        .map(FilterDecodedData::bytes),
        S_FILTER_DCT_DECODE => decode_dct(buf, params),
        S_FILTER_CCITT_FAX => decode_ccitt(
            &buf,
            CCITTFaxDecodeParamsDict::new(
                None,
                params.unwrap_or_else(|| &*empty_dict),
                resolver.unwrap(),
            )?,
        )
        .map(FilterDecodedData::CCITTFaxImage),
        S_FILTER_ASCII85_DECODE => decode_ascii85(&buf, params).map(FilterDecodedData::bytes),
        S_FILTER_ASCII_HEX_DECODE => decode_ascii_hex(&buf).map(FilterDecodedData::bytes),
        S_FILTER_RUN_LENGTH_DECODE => Ok(FilterDecodedData::bytes(decode_run_length(&buf, params))),
        S_FILTER_JPX_DECODE => decode_jpx(buf, params),
        S_FILTER_LZW_DECODE => decode_lzw(
            &buf,
            LZWDeflateDecodeParams::new(params.unwrap_or_else(|| &*empty_dict), resolver)?,
        )
        .map(FilterDecodedData::bytes),
        _ => {
            error!("Unknown filter: {}", filter_name);
            Err(ObjectValueError::UnknownFilter)
        }
    }
}

fn image_transform_color_space(img: DynamicImage, to: &ColorSpace) -> AnyResult<DynamicImage> {
    fn image_color_space(img: &DynamicImage) -> ColorSpace {
        match img {
            DynamicImage::ImageLuma8(_) => ColorSpace::DeviceGray,
            DynamicImage::ImageRgb8(_) => ColorSpace::DeviceRGB,
            _ => todo!("unsupported image color space: {:?}", img),
        }
    }

    fn transform(img: DynamicImage, from: &ColorSpace, to: &ColorSpace) -> AnyResult<DynamicImage> {
        fn convert_cs(img: GrayImage, cs: &impl ColorSpaceTrait<f32>) -> AnyResult<RgbaImage> {
            let mut r = RgbaImage::new(img.width(), img.height());
            for (p, dest_p) in img.pixels().zip(r.pixels_mut()) {
                let color: [u8; 4] = color_to_rgba(cs, &[p[0].into_color_comp()]);
                *dest_p = Rgba(color);
            }
            Ok(r)
        }

        if let (ColorSpace::DeviceGray, ColorSpace::Separation(sep)) = (from, to) {
            return Ok(DynamicImage::ImageRgba8(convert_cs(
                img.into_luma8(),
                sep.as_ref(),
            )?));
        }
        todo!("transform image color space from {:?} to {:?}", from, to);
    }

    let from = image_color_space(&img);
    if &from == to {
        return Ok(img);
    }

    transform(img, &from, to)
}

impl Stream {
    pub fn new(dict: Dictionary, buf_pos: BufPos, id: ObjectId) -> Self {
        Self(dict, buf_pos, id)
    }

    pub fn update_dict(&mut self, f: impl FnOnce(&mut Dictionary)) {
        f(&mut self.0);
    }

    pub fn as_dict(&self) -> &Dictionary {
        &self.0
    }

    pub fn take_dict(self) -> Dictionary {
        self.0
    }

    #[cfg(test)]
    pub fn buf<'a>(&self, buf: &'a [u8]) -> &'a [u8] {
        &buf[self.buf_range(None).unwrap()]
    }

    /// Get stream un-decoded raw data.
    /// `buf` start from indirect object, from xref
    pub fn raw<'a>(&self, resolver: &ObjectResolver<'a>) -> Result<&'a [u8], ObjectValueError> {
        let buf = resolver.stream_data(self.2.id());
        Ok(&buf[self.buf_range(Some(resolver))?])
    }

    fn _decode<'a>(
        &self,
        resolver: &ObjectResolver<'a>,
    ) -> Result<FilterDecodedData<'a>, ObjectValueError> {
        if self.0.contains_key(&KEY_FFILTER) {
            return Err(ObjectValueError::ExternalStreamNotSupported);
        }

        let raw: Cow<'a, [u8]> = self.raw(resolver)?.into();
        decode_stream(&self.0, raw, Some(resolver), None, Some(self.2))
    }

    /// Decode stream data using filter and parameters in stream dictionary.
    /// `image_to_raw` if the stream is image, convert to RawImage.
    pub fn decode<'a>(
        &self,
        resolver: &ObjectResolver<'a>,
    ) -> Result<Cow<'a, [u8]>, ObjectValueError> {
        self._decode(resolver).and_then(|v| v.into_bytes())
    }

    fn buf_range(
        &self,
        resolver: Option<&ObjectResolver>,
    ) -> Result<Range<usize>, ObjectValueError> {
        self.1.range(|| {
            let l = self
                .0
                .get(&sname("Length"))
                .ok_or(ObjectValueError::StreamLengthNotDefined)?;

            match (l, resolver) {
                (Object::Integer(l), _) => Ok(*l as u32),
                (Object::Reference(id), Some(resolver)) => {
                    Ok(resolver.resolve(id.id().id())?.int()? as u32)
                }
                _ => {
                    error!("Length is not Integer or Reference, {:?}", l);
                    Err(ObjectValueError::UnexpectedType)
                }
            }
        })
    }

    /// Decode stream but requires that `Length` field is no ref-id, so no need to use
    /// `ObjectResolver`
    pub fn decode_without_resolve_length<'a>(
        &self,
        buf: &'a [u8],
        encrypt_info: Option<&EncryptInfo>,
    ) -> Result<Cow<'a, [u8]>, ObjectValueError> {
        let data = decode_stream(
            &self.0,
            &buf[self.buf_range(None)?],
            None,
            encrypt_info,
            Some(self.2),
        )?;
        data.into_bytes()
    }

    pub fn decode_image<'a>(
        &self,
        resolver: &ObjectResolver<'a>,
        resources: Option<&ResourceDict<'a, '_>>,
    ) -> Result<DynamicImage, ObjectValueError> {
        let decoded = self._decode(resolver)?;
        let img_dict = ImageDict::new(None, &self.0, resolver)?;
        decode_image(decoded, &img_dict, resolver, resources)
    }
}

type ColorKey = ([u8; 4], [u8; 4]);

/// `range` length is n which is ColorSpace component counts,
/// Convert min and max color into ColorSpace, return (min, max) rgba8
fn color_key_range(range: &Domains, cs: &ColorSpace) -> ColorKey {
    let n = cs.components();
    assert_eq!(range.n(), n);
    assert!(range.n() <= 4);

    let mut min = [0u8; 4];
    let mut max = [0u8; 4];
    for (i, (min, max)) in min.iter_mut().zip(max.iter_mut()).take(n).enumerate() {
        *min = range.0[i].start.to_u8().unwrap();
        *max = range.0[i].end.to_u8().unwrap();
    }
    let min: [_; 4] = convert_color_to(&min[..]);
    let max: [_; 4] = convert_color_to(&max[..]);
    (color_to_rgba(cs, &min[..]), color_to_rgba(cs, &max[..]))
}

/// Return true if rgb color in color_key range inclusive, alpha part not compared.
fn color_matches_color_key(color_key: ColorKey, color: [u8; 4]) -> bool {
    color_key.0[0..2] <= color[0..2] && color[0..2] <= color_key.1[0..2]
}

#[cfg(test)]
mod tests;
