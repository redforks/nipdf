use nom::{
    branch::alt,
    combinator::value,
    error::{ErrorKind, ParseError as NomParseError},
    multi::many0_count,
    sequence::{delimited, preceded, terminated},
    IResult, InputTakeAtPosition, Parser,
};

mod file;
mod object;

pub use file::*;
pub use object::*;

// Set `nom::error:VerboseError<&'a[u8]>` for detail error
#[cfg(not(debug_assertions))]
pub type ParseError<'a> = nom::error::Error<&'a [u8]>;
#[cfg(debug_assertions)]
pub type ParseError<'a> = nom::error::VerboseError<&'a [u8]>;
pub type ParseResult<'a, O, E = ParseError<'a>> = IResult<&'a [u8], O, E>;

/// Error at file struct level.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum FileError {
    #[error("Unsupported version: {0}")]
    UnsupportedVersion(String),
    #[error("No enough data")]
    NoEnoughData,
}

fn comment(buf: &[u8]) -> ParseResult<'_, ()> {
    let (buf, _) = nom::bytes::complete::tag(b"%")(buf)?;
    let (buf, content) = nom::bytes::complete::is_not("\n\r")(buf)?;
    if content.starts_with(b"PDF-") || content.starts_with(b"%EOF") {
        return Err(nom::Err::Error(ParseError::from_error_kind(
            buf,
            ErrorKind::Fail,
        )));
    }
    Ok((buf, ()))
}

fn whitespace1<T, E: nom::error::ParseError<T>>(input: T) -> IResult<T, T, E>
where
    T: InputTakeAtPosition<Item = u8>,
{
    // in PDF 32000-1:2008 7.2.2 '\0' is whitespace, but in 4.46 '\0' is
    // not listed as whitespace. Exclude '\0' because after `stream` tag,
    // '\0' maybe part of stream content.
    input.split_at_position1_complete(
        |c| !(c == b' ' || c == b'\t' || c == b'\r' || c == b'\n' || c == b'\x0C'),
        nom::error::ErrorKind::MultiSpace,
    )
}

pub(crate) fn whitespace_or_comment(input: &[u8]) -> ParseResult<'_, ()> {
    value((), many0_count(alt((value((), whitespace1), comment))))(input)
}

pub(crate) fn ws_prefixed<'a, F, O>(inner: F) -> impl FnMut(&'a [u8]) -> ParseResult<'_, O>
where
    F: Parser<&'a [u8], O, ParseError<'a>>,
{
    preceded(whitespace_or_comment, inner)
}

/// A combinator that takes a parser `inner` and produces a parser that also consumes both leading and
/// trailing whitespace, returning the output of `inner`.
fn ws<'a, F, O>(inner: F) -> impl FnMut(&'a [u8]) -> ParseResult<'_, O>
where
    F: Parser<&'a [u8], O, ParseError<'a>>,
{
    delimited(whitespace_or_comment, inner, whitespace_or_comment)
}

pub(crate) fn ws_terminated<'a, F, O>(inner: F) -> impl FnMut(&'a [u8]) -> ParseResult<'_, O>
where
    F: Parser<&'a [u8], O, ParseError<'a>>,
{
    terminated(inner, whitespace_or_comment)
}

#[cfg(test)]
mod tests;
