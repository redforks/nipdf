use nom::{
    IResult, InputTakeAtPosition, Parser,
    branch::alt,
    combinator::{opt, value},
    error::{ErrorKind, ParseError as NomParseError},
    multi::many0_count,
    sequence::{delimited, preceded, terminated},
};
use snafu::Snafu;

mod file;
mod object;
mod object_now;
pub use file::*;
pub use object::*;
use winnow::{Parser as _, stream::ContainsToken};

// Set `nom::error:VerboseError<&'a[u8]>` for detail error
#[cfg(not(debug_assertions))]
pub type ParseError<'a> = nom::error::Error<&'a [u8]>;
#[cfg(debug_assertions)]
pub type ParseError<'a> = nom::error::VerboseError<&'a [u8]>;
pub type ParseResult<'a, O, E = ParseError<'a>> = IResult<&'a [u8], O, E>;

/// Error at file struct level.
#[derive(Clone, PartialEq, Eq, Debug, Snafu)]
pub enum FileError {
    #[snafu(display("Unsupported version: {version}"))]
    UnsupportedVersion { version: String },
    #[snafu(display("No enough data"))]
    NoEnoughData,
}

fn comment(buf: &[u8]) -> ParseResult<'_, ()> {
    let (buf, _) = nom::bytes::complete::tag(b"%")(buf)?;
    let (buf, content) = opt(nom::bytes::complete::is_not("\n\r"))(buf)?;
    if let Some(content) = content {
        if content.starts_with(b"PDF-") || content.starts_with(b"%EOF") {
            return Err(nom::Err::Error(ParseError::from_error_kind(
                buf,
                ErrorKind::Fail,
            )));
        }
    }
    Ok((buf, ()))
}

pub(crate) fn is_white_space(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == b'\n' || b == b'\x0C' || b == b'\r' || b == b'\0'
}

/// Return eol parser.
///
/// EOL is '\n', '\r', or "\r\n"
fn eol_now<'a>() -> impl winnow::Parser<&'a [u8], (), crate::ParserError> {
    use winnow::{
        combinator::{cond, opt},
        token::one_of,
    };
    one_of([b'\n', b'\r'])
        .flat_map(|v| cond(v == b'\r', opt(b'\n')))
        .void()
}

/// Return comment parser. Parser returns comment string, `%` prefix and newline suffix not
/// included.
fn comment_now<'a>() -> impl winnow::Parser<&'a [u8], &'a [u8], crate::ParserError> {
    use winnow::{combinator::delimited, token::take_till};
    delimited(b'%', take_till(0.., [b'\n', b'\r']), eol_now())
}

/// Return parser that parse one of whitespace characters.
///
/// in PDF 32000-1:2008 7.2.2 '\0' is whitespace, but in 4.46 '\0' is
/// not listed as whitespace. Exclude '\0' because after `stream` tag,
/// '\0' maybe part of stream content.
fn whitespace_now<'a>() -> impl winnow::Parser<&'a [u8], u8, crate::ParserError> {
    use winnow::token::one_of;
    one_of(is_whitespace())
}

fn is_whitespace() -> impl ContainsToken<u8> {
    b" \t\r\n\x0C"
}

/// Return parser that parse one or more whitespace and/or comments.
pub fn ws_now<'a>() -> impl winnow::Parser<&'a [u8], (), crate::ParserError> {
    use winnow::combinator::{alt, repeat};
    repeat(1.., alt((whitespace_now().void(), comment_now().void())))
}

#[allow(clippy::needless_pass_by_value)]
fn whitespace1<T, E: nom::error::ParseError<T>>(input: T) -> IResult<T, T, E>
where
    T: InputTakeAtPosition<Item = u8>,
{
    // in PDF 32000-1:2008 7.2.2 '\0' is whitespace, but in 4.46 '\0' is
    // not listed as whitespace. Exclude '\0' because after `stream` tag,
    // '\0' maybe part of stream content.
    input.split_at_position1_complete(
        |c| !(c == b' ' || c == b'\t' || c == b'\r' || c == b'\n' || c == b'\x0C'),
        ErrorKind::MultiSpace,
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

/// A combinator that takes a parser `inner` and produces a parser that also consumes both leading
/// and trailing whitespace, returning the output of `inner`.
pub(crate) fn ws<'a, F, O>(inner: F) -> impl FnMut(&'a [u8]) -> ParseResult<'_, O>
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
