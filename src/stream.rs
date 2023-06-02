use lopdf::Stream;

#[derive(thiserror::Error, Debug)]
pub enum DecodeError {
    #[error("Unknown filter {0}")]
    UnknownFilter(String),
    #[error("External stream not supported")]
    ExternalStreamNotSupported,
}

/// Decode stream content by filters defined in `stream` dict.
pub fn decode(stream: &Stream) -> Result<Vec<u8>, DecodeError> {
    todo!()
}
