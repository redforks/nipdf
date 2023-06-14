use nom::{error::FromExternalError, IResult};

#[derive(PartialEq, Debug, thiserror::Error)]
pub enum PdfParseError<I, E>
where
    E: nom::error::ParseError<I> + std::fmt::Debug + PartialEq,
{
    #[error("nom parse error: {0}")]
    NomError(#[from] E),

    #[error("Stream require length field in info dict")]
    StreamRequireLength,
    #[error("Stream length type is not integer")]
    StreamInvalidLengthType,

    #[error("phantom for generic type I, Not used")]
    Phantom(I),
}

impl<I, E1, E2> FromExternalError<I, E1> for PdfParseError<I, E2>
where
    E2: nom::error::FromExternalError<I, E1>
        + nom::error::ParseError<I>
        + std::fmt::Debug
        + PartialEq,
{
    fn from_external_error(input: I, kind: nom::error::ErrorKind, e: E1) -> Self {
        Self::NomError(E2::from_external_error(input, kind, e))
    }
}

impl<I, E> nom::error::ParseError<I> for PdfParseError<I, E>
where
    E: nom::error::ParseError<I> + std::fmt::Debug + PartialEq,
{
    fn from_error_kind(input: I, kind: nom::error::ErrorKind) -> Self {
        Self::NomError(E::from_error_kind(input, kind))
    }

    fn append(_input: I, _kind: nom::error::ErrorKind, other: Self) -> Self {
        other
    }
}

// Set `nom::error:VerboseError<&'a[u8]>` for detail error
pub type ParseError<'a> = PdfParseError<&'a [u8], nom::error::Error<&'a [u8]>>;
pub type ParseResult<'a, O, E = ParseError<'a>> = IResult<&'a [u8], O, E>;

/// Error at file struct level.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum FileError {
    #[error("Unsupported version: {0}")]
    UnsupportedVersion(String),
    #[error("No enough data")]
    NoEnoughData,
}

mod file;
mod object;

pub use object::{
    parse_complete_array, parse_complete_dict, parse_complete_indirected_object,
    parse_complete_object, parse_complete_reference, parse_complete_stream,
};
