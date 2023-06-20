use std::{
    borrow::{Borrow, Cow},
    iter::repeat,
    str::from_utf8,
};

use log::error;

use super::{Dictionary, Name, Object, ObjectValueError};

const KEY_FILTER: &[u8] = b"Filter";
const KEY_FILTER_PARAMS: &[u8] = b"DecodeParms";
const KEY_FFILTER: &[u8] = b"FFilter";

#[derive(Clone, PartialEq, Debug)]
pub struct Stream<'a>(pub Dictionary<'a>, pub &'a [u8]);

fn decode_flate(buf: &[u8], params: Option<&Dictionary>) -> Result<Vec<u8>, ObjectValueError> {
    assert!(params.is_none(), "TODO: handle params of FlateDecode");

    use flate2::bufread::{DeflateDecoder, ZlibDecoder};
    use std::io::Read;

    let mut output = Vec::with_capacity(buf.len() * 2);
    let mut decoder = ZlibDecoder::new(buf);
    decoder
        .read_to_end(&mut output)
        .or_else(|_| DeflateDecoder::new(buf).read_to_end(&mut output))
        .map_err(|err| {
            error!(
                "Failed to decode FlateDecode using DeflateDecoder: {:?}",
                err
            );
            ObjectValueError::FilterDecodeError
        })?;

    // let mut file = std::fs::File::create("/tmp/stream").unwrap();
    // file.write_all(&buf).unwrap();
    // drop(file);
    Ok(output)
}

fn filter<'a>(
    buf: Cow<'a, [u8]>,
    filter_name: &[u8],
    params: Option<&Dictionary<'a>>,
) -> Result<Cow<'a, [u8]>, ObjectValueError> {
    match filter_name {
        b"FlateDecode" => decode_flate(&buf, params).map(Cow::Owned),
        b"DCTDecode" => Ok(buf),
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
