use lopdf::{Object, Stream};

/// Stream object dict key `Filter`.
pub const KEY_FILTER: &[u8] = b"Filter";
/// Stream object dict key `DecodeParms`.
pub const KEY_DECODE_PARMS: &[u8] = b"DecodeParms";
/// Stream object dict key `FFilter`.
pub const KEY_FFILTER: &[u8] = b"FFilter";
/// Stream object dict key `FDecodeParms`.
pub const KEY_FDECODE_PARMS: &[u8] = b"FDecodeParms";

#[cfg(test)]
/// Stream filter name of zero decoder, that replace all bytes to zero (\0),
/// used in unit tests
pub const FILTER_ZERO_DECODER: &[u8] = b"zero";

#[derive(thiserror::Error, Debug)]
pub enum DecodeError {
    #[error("Unknown filter {0}")]
    UnknownFilter(String),
    #[error("External stream not supported")]
    ExternalStreamNotSupported,
}

/// Decode stream content by filters defined in `stream` dict.
pub fn decode(stream: &Stream) -> Result<Vec<u8>, DecodeError> {
    #[cfg(test)]
    if let Ok(filter_name) = stream.dict.get(KEY_FILTER) {
        match filter_name {
            Object::Name(name) if name == FILTER_ZERO_DECODER => {
                return Ok(stream.content.iter().map(|_| 0u8).collect());
            }
            _ => {
                todo!()
            }
        }
    }
    todo!()
}
