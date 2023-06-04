use anyhow::Result as AnyResult;
use lopdf::{Dictionary, Object, Stream};
use std::io::Read;
use std::{borrow::Cow, iter::repeat, str::from_utf8};

#[cfg(test)]
fn zero_decoder<'a>(data: Cow<'a, [u8]>, _params: Option<&Dictionary>) -> AnyResult<Cow<'a, [u8]>> {
    Ok(Cow::from(vec![0; data.len()]))
}

#[cfg(test)]
fn inc_decoder<'a>(data: Cow<'a, [u8]>, params: Option<&Dictionary>) -> AnyResult<Cow<'a, [u8]>> {
    let step = params.map_or(1, |p| {
        p.get(super::FILTER_INC_DECODER_STEP_PARAM)
            .map_or(1u8, |v| {
                if let Object::Integer(i) = v {
                    *i as u8
                } else {
                    panic!("Invalid step parameter type")
                }
            })
    });
    let mut buf = Vec::with_capacity(data.len());
    for b in data.iter() {
        buf.push(b + step);
    }
    Ok(Cow::from(buf))
}

fn flate_decode<'a>(data: Cow<'a, [u8]>, params: Option<&Dictionary>) -> AnyResult<Cow<'a, [u8]>> {
    assert!(
        params.is_none(),
        "FlateDecode params support not implemented"
    );
    let mut decoder = flate2::bufread::ZlibDecoder::new(data.as_ref());
    let mut buf = Vec::new();
    decoder.read_to_end(&mut buf)?;
    Ok(Cow::from(buf))
}

fn filter<'a>(
    data: Cow<'a, [u8]>,
    filter_name: &[u8],
    params: Option<&Dictionary>,
) -> Result<Cow<'a, [u8]>, DecodeError> {
    match filter_name {
        #[cfg(test)]
        super::FILTER_ZERO_DECODER => Ok(zero_decoder(data, params)?),
        #[cfg(test)]
        super::FILTER_INC_DECODER => Ok(inc_decoder(data, params)?),
        super::FILTER_FLATE_DECODE => Ok(flate_decode(data, params)?),
        _ => Err(DecodeError::UnknownFilter(
            from_utf8(filter_name).unwrap().to_string(),
        )),
    }
}

#[derive(thiserror::Error, Debug)]
pub enum DecodeError {
    #[error("Unknown filter {0}")]
    UnknownFilter(String),
    #[error("External stream not supported")]
    ExternalStreamNotSupported,
    #[error("Filter error")]
    FilterError(#[from] anyhow::Error),
    #[error("Filter and params mismatch")] // more than one filter and params not array
    FilterAndParamsMismatch,
    #[error("Invalid filter object type")]
    InvalidFilterObjectType,
    #[error("Invalid params object type")]
    InvalidParamsObjectType,
}

/// Iterate over filters and their parameters of `stream_dict`.
fn iter_filter(
    stream_dict: &Dictionary,
) -> Result<impl Iterator<Item = (&[u8], Option<&Dictionary>)>, DecodeError> {
    let filters = stream_dict.get(super::KEY_FILTER).map_or_else(
        |_| Ok(vec![]),
        |v| match v {
            Object::Array(vals) => vals
                .iter()
                .map(|v| {
                    v.as_name()
                        .map_err(|_| DecodeError::InvalidFilterObjectType)
                })
                .collect(),
            Object::Name(s) => Ok(vec![s]),
            _ => Err(DecodeError::InvalidFilterObjectType),
        },
    )?;
    let params = stream_dict.get(super::KEY_DECODE_PARMS).map_or_else(
        |_| Ok(vec![]),
        |v| match v {
            Object::Null => Ok(vec![]),
            Object::Array(vals) => vals
                .iter()
                .map(|v| match v {
                    Object::Null => Ok(None),
                    Object::Dictionary(dict) => Ok(Some(dict)),
                    _ => Err(DecodeError::InvalidParamsObjectType),
                })
                .collect(),
            Object::Dictionary(dict) => Ok(vec![Some(dict)]),
            _ => Err(DecodeError::InvalidParamsObjectType),
        },
    )?;
    Ok(filters
        .into_iter()
        .zip(params.into_iter().chain(repeat(None))))
}

/// Decode stream content by filters defined in `stream` dict.
pub fn decode(stream: &Stream) -> Result<Cow<[u8]>, DecodeError> {
    if stream.dict.has(super::KEY_FFILTER) {
        return Err(DecodeError::ExternalStreamNotSupported);
    }

    let mut buf = Cow::from(stream.content.as_slice());
    for (filter_name, params) in iter_filter(&stream.dict)? {
        buf = filter(buf, filter_name, params)?;
    }
    Ok(buf)
}

#[cfg(test)]
mod tests;
