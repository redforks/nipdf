use anyhow::Result as AnyResult;
use lopdf::Stream;
use once_cell::unsync::Lazy;
use std::{borrow::Cow, str::from_utf8};

use lopdf::Dictionary;

pub trait Filter {
    fn filter<'a>(&self, data: Cow<'a, [u8]>, params: &Dictionary) -> AnyResult<Cow<'a, [u8]>>;
}

impl<F: for<'b> Fn(Cow<'b, [u8]>, &Dictionary) -> AnyResult<Cow<'b, [u8]>> + 'static> Filter for F {
    fn filter<'a>(
        &self,
        data: Cow<'a, [u8]>,
        params: &Dictionary,
    ) -> Result<Cow<'a, [u8]>, anyhow::Error> {
        self(data, params)
    }
}

#[cfg(test)]
fn zero_decoder<'a>(data: Cow<'a, [u8]>, _params: &Dictionary) -> AnyResult<Cow<'a, [u8]>> {
    Ok(Cow::from(vec![0; data.len()]))
}

#[cfg(test)]
fn inc_decoder<'a>(data: Cow<'a, [u8]>, params: &Dictionary) -> AnyResult<Cow<'a, [u8]>> {
    use lopdf::Object;

    let step = params
        .get(super::FILTER_INC_DECODER_STEP_PARAM)
        .map_or(1u8, |v| {
            if let Object::Integer(i) = v {
                *i as u8
            } else {
                panic!("Invalid step parameter type")
            }
        });
    let mut buf = Vec::with_capacity(data.len());
    for b in data.iter() {
        buf.push(b + step);
    }
    Ok(Cow::from(buf))
}

fn create_filter(name: &[u8]) -> Result<Box<dyn Filter>, DecodeError> {
    match name {
        #[cfg(test)]
        super::FILTER_ZERO_DECODER => Ok(Box::new(zero_decoder)),
        #[cfg(test)]
        super::FILTER_INC_DECODER => Ok(Box::new(inc_decoder)),
        _ => Err(DecodeError::UnknownFilter(
            from_utf8(name).unwrap().to_string(),
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
}

/// Decode stream content by filters defined in `stream` dict.
pub fn decode(stream: &Stream) -> Result<Vec<u8>, DecodeError> {
    if stream.dict.has(super::KEY_FFILTER) {
        return Err(DecodeError::ExternalStreamNotSupported);
    }

    let Ok(filters) = stream.filters() else {
        return Ok(stream.content.clone());
    };

    let empty_dict = Dictionary::new();
    let params = Lazy::new(|| {
        stream.dict.get(super::KEY_DECODE_PARMS).map_or_else(
            |_| &empty_dict,
            |v| v.as_dict().expect("DecodeParms should be dict"),
        )
    });
    let mut buf = Cow::from(stream.content.as_slice());
    for filter_name in filters {
        let f = create_filter(filter_name.as_bytes())?;
        buf = f.filter(buf, &params)?;
    }
    Ok(buf.into_owned())
}

#[cfg(test)]
mod tests;
