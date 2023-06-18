use std::ops::{RangeBounds, RangeTo};

use nom::{
    character::complete::multispace0,
    error::FromExternalError,
    sequence::{delimited, preceded, terminated},
    IResult, Parser,
};

#[derive(PartialEq, Debug, thiserror::Error)]
pub enum PdfParseError<I, E>
where
    E: nom::error::ParseError<I> + std::fmt::Debug + PartialEq,
{
    #[error("nom parse error: {0:?}")]
    NomError(E),

    #[error("Stream require length field in info dict")]
    StreamRequireLength,
    #[error("Stream length type is not integer")]
    StreamInvalidLengthType,

    #[error("Not valid pdf file")]
    InvalidFile,

    #[error("phantom for generic type I, Not used")]
    Phantom(I),
}

impl<'a, E1, E2> FromExternalError<&'a [u8], E1> for PdfParseError<&'a [u8], E2>
where
    E2: nom::error::FromExternalError<&'a [u8], E1>
        + nom::error::ParseError<&'a [u8]>
        + std::fmt::Debug
        + PartialEq,
{
    fn from_external_error(input: &'a [u8], kind: nom::error::ErrorKind, e: E1) -> Self {
        Self::NomError(E2::from_external_error(
            &input[..20.min(input.len())],
            kind,
            e,
        ))
    }
}

impl<'a, E> nom::error::ParseError<&'a [u8]> for PdfParseError<&'a [u8], E>
where
    E: nom::error::ParseError<&'a [u8]> + std::fmt::Debug + PartialEq,
{
    fn from_error_kind(input: &'a [u8], kind: nom::error::ErrorKind) -> Self {
        Self::NomError(E::from_error_kind(&input[..20.min(input.len())], kind))
    }

    fn append(_input: &'a [u8], _kind: nom::error::ErrorKind, other: Self) -> Self {
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

pub use file::*;
pub use object::*;

fn ws_prefixed<'a, F, O>(inner: F) -> impl FnMut(&'a [u8]) -> ParseResult<'_, O>
where
    F: Parser<&'a [u8], O, ParseError<'a>>,
{
    preceded(multispace0, inner)
}

/// A combinator that takes a parser `inner` and produces a parser that also consumes both leading and
/// trailing whitespace, returning the output of `inner`.
fn ws<'a, F, O>(inner: F) -> impl FnMut(&'a [u8]) -> ParseResult<'_, O>
where
    F: Parser<&'a [u8], O, ParseError<'a>>,
{
    delimited(multispace0, inner, multispace0)
}

fn ws_terminated<'a, F, O>(inner: F) -> impl FnMut(&'a [u8]) -> ParseResult<'_, O>
where
    F: Parser<&'a [u8], O, ParseError<'a>>,
{
    terminated(inner, multispace0)
}
