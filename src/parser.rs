use nom::IResult;

// Set `= nom::error:VerboseError<&'a[u8]>` for detail error
pub type ParseError<'a> = nom::error::Error<&'a [u8]>;
pub type ParseResult<'a, O, E = ParseError<'a>> = IResult<&'a [u8], O, E>;

#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum LogicParseError {
    #[error("Unsupported version: {0}")]
    UnsupportedVersion(String),
    #[error("No enough data")]
    NoEnoughData,
}

mod file;
mod object;
