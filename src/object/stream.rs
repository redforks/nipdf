use std::{
    borrow::{Borrow, Cow},
    fmt::Display,
    io::Cursor,
    iter::repeat,
    str::from_utf8,
};

use image::{write_buffer_with_format, ImageFormat};
use log::error;
use once_cell::unsync::Lazy;

use super::{Dictionary, Name, Object, ObjectValueError};

const KEY_FILTER: &[u8] = b"Filter";
const KEY_FILTER_PARAMS: &[u8] = b"DecodeParms";
const KEY_FFILTER: &[u8] = b"FFilter";

const FILTER_CCITT_FAX: &str = "CCITTFaxDecode";
const B_FILTER_CCITT_FAX: &[u8] = FILTER_CCITT_FAX.as_bytes();
const FILTER_DCT_DECODE: &str = "DCTDecode";
const B_FILTER_DCT_DECODE: &[u8] = FILTER_DCT_DECODE.as_bytes();

#[derive(Clone, PartialEq, Debug)]
pub struct Stream<'a>(pub Dictionary<'a>, pub &'a [u8]);

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
        "FlateDecode",
    )?;

    // let mut file = std::fs::File::create("/tmp/stream").unwrap();
    // file.write_all(&buf).unwrap();
    // drop(file);
    Ok(output)
}

fn decode_dct(buf: &[u8], params: Option<&Dictionary>) -> Result<Vec<u8>, ObjectValueError> {
    assert!(
        params.is_none(),
        "TODO: handle params of {}",
        FILTER_DCT_DECODE
    );
    use jpeg_decoder::Decoder;
    let mut decoder = Decoder::new(buf);
    handle_filter_error(decoder.decode(), FILTER_DCT_DECODE)
}

struct CCITTFaxDecodeParams<'a: 'b, 'b>(&'b Dictionary<'a>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CCITTFGroup {
    Group3_1D,
    Group3_2D(i32),
    Group4,
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

fn decode_ccitt<'a: 'b, 'b>(
    input: &[u8],
    params: Option<&'b Dictionary<'a>>,
) -> Result<Vec<u8>, ObjectValueError> {
    _decode_ccitt(input, params).map(|(buf, _meta)| buf)
}

fn _decode_ccitt<'a: 'b, 'b>(
    input: &[u8],
    params: Option<&'b Dictionary<'a>>,
) -> Result<(Vec<u8>, (u32, u32)), ObjectValueError> {
    use crate::ccitt::decode;

    let empty_params = Lazy::new(Dictionary::new);
    let params = CCITTFaxDecodeParams(params.unwrap_or_else(|| Lazy::force(&empty_params)));
    let image = handle_filter_error(
        decode(input, params.columns(), Some(params.rows() as usize)),
        FILTER_CCITT_FAX,
    )?;
    assert_eq!(
        params.rows() as usize,
        image.len() / params.columns() as usize
    );
    Ok((image, (params.columns() as u32, params.rows() as u32)))
}

fn filter<'a: 'b, 'b>(
    buf: Cow<'a, [u8]>,
    filter_name: &[u8],
    params: Option<&'b Dictionary<'a>>,
) -> Result<Cow<'a, [u8]>, ObjectValueError> {
    match filter_name {
        b"FlateDecode" => decode_flate(&buf, params).map(Cow::Owned),
        B_FILTER_DCT_DECODE => decode_dct(&buf, params).map(Cow::Owned),
        B_FILTER_CCITT_FAX => decode_ccitt(&buf, params).map(Cow::Owned),
        _ => {
            error!("Unknown filter: {}", from_utf8(filter_name).unwrap());
            Err(ObjectValueError::UnknownFilter)
        }
    }
}

pub struct Image {
    pub format: ImageFormat,
    pub data: Vec<u8>,
}

fn ensure_last_filter<T>(v: T, has_next: bool, filter_name: &str) -> Result<T, ObjectValueError> {
    if !has_next {
        Ok(v)
    } else {
        error!("should no other filter after {}", filter_name,);
        Err(ObjectValueError::FilterDecodeError)
    }
}

fn ccitt_to_image(buf: &[u8], params: Option<&Dictionary<'_>>) -> Result<Image, ObjectValueError> {
    let (data, (w, h)) = _decode_ccitt(buf, params)?;
    let mut cursor = Cursor::new(Vec::new());
    write_buffer_with_format(
        &mut cursor,
        &data,
        w,
        h,
        image::ColorType::L8,
        ImageFormat::Png,
    )
    .unwrap();
    Ok(Image {
        format: ImageFormat::Png,
        data: cursor.into_inner(),
    })
}

impl<'a> Stream<'a> {
    /// Decode stream data using filter and parameters in stream dictionary.
    pub fn decode(&self) -> Result<Cow<[u8]>, ObjectValueError> {
        let mut buf = Cow::Borrowed(self.1);
        for (filter_name, params) in self.iter_filter()? {
            buf = filter(buf, filter_name, params)?;
        }
        Ok(buf)
    }

    pub fn to_image(&self) -> Result<Image, ObjectValueError> {
        let mut buf = Cow::Borrowed(self.1);
        let mut iter = self.iter_filter()?;
        for (filter_name, params) in iter.by_ref() {
            match filter_name {
                B_FILTER_DCT_DECODE => {
                    return ensure_last_filter(
                        Image {
                            format: ImageFormat::Jpeg,
                            data: buf.into(),
                        },
                        iter.next().is_some(),
                        FILTER_DCT_DECODE,
                    );
                }
                B_FILTER_CCITT_FAX => {
                    return ensure_last_filter(
                        ccitt_to_image(&buf, params)?,
                        iter.next().is_some(),
                        FILTER_CCITT_FAX,
                    );
                }
                _ => {
                    buf = filter(buf, filter_name, params)?;
                }
            }
        }
        Err(ObjectValueError::StreamNotImage)
    }

    fn iter_filter(
        &self,
    ) -> Result<impl Iterator<Item = (&[u8], Option<&Dictionary<'a>>)>, ObjectValueError> {
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
                Object::Name(n) => Ok(vec![n.0.borrow()]),
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
