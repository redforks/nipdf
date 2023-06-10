use nom::IResult;

// Set `E = nom::error:VerboseError<&'a[u8]>` for detail error
pub type ParseResult<'a, O, E = nom::error::Error<&'a [u8]>> = IResult<&'a [u8], O, E>;

#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum LogicParseError {
    #[error("Unsupported version: {0}")]
    UnsupportedVersion(String),
}

mod file;
