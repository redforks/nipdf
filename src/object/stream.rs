use std::{
    borrow::{Borrow, Cow},
    fmt::Display,
    iter::repeat,
    str::from_utf8,
};

use log::error;
use once_cell::unsync::Lazy;

use super::{Dictionary, Name, Object, ObjectValueError};

const KEY_FILTER: &[u8] = b"Filter";
const KEY_FILTER_PARAMS: &[u8] = b"DecodeParms";
const KEY_FFILTER: &[u8] = b"FFilter";

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
    assert!(params.is_none(), "TODO: handle params of DCTDecode");
    use jpeg_decoder::Decoder;
    let mut decoder = Decoder::new(buf);
    handle_filter_error(decoder.decode(), "DCTDecode")
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
    use fax::{
        decoder::{decode_g4, pels},
        Color,
    };
    // let empty_params = Dictionary::default();
    let empty_params = Lazy::new(|| Dictionary::default());
    let params = CCITTFaxDecodeParams(params.unwrap_or_else(|| &empty_params));
    assert!(params.k() == CCITTFGroup::Group4, "CCITT: mode supported");
    let columns = params.columns();
    let rows = params.rows();
    let mut buf = Vec::with_capacity(columns as usize * rows as usize);
    let height = if rows == 0 { None } else { Some(rows) };
    decode_g4(input.iter().cloned(), columns, height, |line| {
        buf.extend(pels(line, columns as u16).map(|c| match c {
            Color::Black => 0,
            Color::White => 255,
        }));
        assert_eq!(
            buf.len() % columns as usize,
            0,
            "len={}, columns={}",
            buf.len(),
            columns
        );
    })
    .ok_or(ObjectValueError::FilterDecodeError)?;
    Ok(buf)
}

fn filter<'a: 'b, 'b>(
    buf: Cow<'a, [u8]>,
    filter_name: &[u8],
    params: Option<&'b Dictionary<'a>>,
) -> Result<Cow<'a, [u8]>, ObjectValueError> {
    match filter_name {
        b"FlateDecode" => decode_flate(&buf, params).map(Cow::Owned),
        b"DCTDecode" => decode_dct(&buf, params).map(Cow::Owned),
        b"CCITTFaxDecode" => decode_ccitt(&buf, params).map(Cow::Owned),
        _ => {
            error!("Unknown filter: {}", from_utf8(filter_name).unwrap());
            Err(ObjectValueError::UnknownFilter)
        }
    }
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
