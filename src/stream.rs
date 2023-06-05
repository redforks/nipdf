mod filter;
pub use filter::{decode, DecodeError};

/// Stream object dict key `Filter`.
pub const KEY_FILTER: &[u8] = b"Filter";
/// Stream object dict key `DecodeParms`.
pub const KEY_DECODE_PARMS: &[u8] = b"DecodeParms";
/// Stream object dict key `FFilter`.
pub const KEY_FFILTER: &[u8] = b"FFilter";
/// Stream object dict key `FDecodeParms`.
pub const KEY_FDECODE_PARMS: &[u8] = b"FDecodeParms";

/// Stream filter name of zero decoder, that replace all bytes to zero (\0),
/// used in unit tests
#[cfg(test)]
pub const FILTER_ZERO_DECODER: &[u8] = b"zero";

/// Param name of [FILTER_INC_DECODER] filter, defines increment step,
#[cfg(test)]
pub const FILTER_INC_DECODER_STEP_PARAM: &[u8] = b"inc-step";
/// Stream filter name of increment decoder, that increment all bytes by one or use
/// step defined in [FILTER_INC_DECODER_STEP_PARAM] param, used in unit tests.
#[cfg(test)]
pub const FILTER_INC_DECODER: &[u8] = b"inc";

pub const FILTER_ASCII_HEX_DECODE: &[u8] = b"ASCIIHexDecode";
pub const FILTER_FLATE_DECODE: &[u8] = b"FlateDecode";
