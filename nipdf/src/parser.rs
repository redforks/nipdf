use snafu::Snafu;

mod file;
mod object;
pub(crate) use file::{header as header_parser, parse_frame_set, parse_xref_stream};
#[cfg(test)]
pub(crate) use object::hex_string;
pub(crate) use object::{
    dict, dict_body, indirect_object_def, object, object_id, object_inside_page_stream,
};
use winnow::{
    Parser,
    combinator::{alt, cond, delimited, eof, opt, preceded, repeat},
    error::ParserError,
    stream::{Compare, ContainsToken, Stream, StreamIsPartial},
    token::{one_of, take_till},
};

/// Error at file struct level.
#[derive(Clone, PartialEq, Eq, Debug, Snafu)]
pub enum FileError {
    #[snafu(display("Unsupported version: {version}"))]
    UnsupportedVersion { version: String },
    #[snafu(display("No enough data"))]
    NoEnoughData,
}

pub(crate) fn is_white_space(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == b'\n' || b == b'\x0C' || b == b'\r' || b == b'\0'
}

/// Return eol parser that has 3 alternative: '\n', '\r', or "\r\n"
pub(crate) fn eol3<'a, S, E>() -> impl Parser<S, (), E> + use<S, E>
where
    S: Stream<Token = u8, Slice = &'a [u8]> + StreamIsPartial + Compare<u8>,
    E: ParserError<S> + 'a,
{
    one_of([b'\n', b'\r'])
        .flat_map(|v| cond(v == b'\r', opt(b'\n')))
        .void()
}

/// Return eol parser that has 2 alternative: '\n', or '\r\n'
fn eol2<'a, S, E>() -> impl Parser<S, (), E> + use<S, E>
where
    S: Stream<Token = u8, Slice = &'a [u8]> + StreamIsPartial + Compare<u8>,
    E: ParserError<S> + 'a,
{
    one_of([b'\n', b'\r'])
        .flat_map(|v| cond(v == b'\r', opt(b'\n')))
        .void()
}

/// Return comment parser. Parser returns comment string, `%` prefix and newline suffix not
/// included.
fn comment<'a, S, E>() -> impl Parser<S, &'a [u8], E>
where
    S: Stream<Token = u8, Slice = &'a [u8]> + StreamIsPartial + Compare<u8>,
    E: ParserError<S> + 'a,
{
    delimited(
        b'%',
        take_till(0.., [b'\n', b'\r']),
        alt((eol3().void(), eof.void())),
    )
}

/// Return parser that parse one of whitespace characters.
///
/// in PDF 32000-1:2008 7.2.2 '\0' is whitespace, but in 4.46 '\0' is
/// not listed as whitespace. Exclude '\0' because after `stream` tag,
/// '\0' maybe part of stream content.
pub fn whitespace<'a, S, E>() -> impl Parser<S, u8, E> + 'a
where
    S: Stream<Token = u8, Slice = &'a [u8]> + StreamIsPartial + Compare<u8> + 'a,
    E: ParserError<S> + 'a,
{
    one_of(is_whitespace())
}

fn is_whitespace() -> impl ContainsToken<u8> {
    b" \t\r\n\x0C\0"
}

/// Parses a Whitespace or a comment.
pub fn wsc<'a, S, E>() -> impl Parser<S, (), E> + 'a
where
    S: Stream<Token = u8, Slice = &'a [u8]> + StreamIsPartial + Compare<u8> + 'a,
    E: ParserError<S> + 'a,
{
    alt((whitespace().void(), comment().void()))
}

/// Matches 0 or more whitespace or comments.
pub fn wsc0<'a, S, E>() -> impl Parser<S, (), E> + 'a
where
    S: Stream<Token = u8, Slice = &'a [u8]> + StreamIsPartial + Compare<u8> + 'a,
    E: ParserError<S> + 'a,
{
    repeat::<_, _, (), _, _>(0.., wsc()).void()
}

/// Matches 0 or one whitespace or comments.
pub fn wsc0_or_1<'a, S, E>() -> impl Parser<S, (), E> + 'a
where
    S: Stream<Token = u8, Slice = &'a [u8]> + StreamIsPartial + Compare<u8> + 'a,
    E: ParserError<S> + 'a,
{
    repeat::<_, _, (), _, _>(0..1, wsc()).void()
}

/// Matches 1 or more whitespace or comments.
pub fn wsc1<'a, S, E>() -> impl Parser<S, (), E> + 'a
where
    S: Stream<Token = u8, Slice = &'a [u8]> + StreamIsPartial + Compare<u8> + 'a,
    E: ParserError<S> + 'a,
{
    repeat::<_, _, (), _, _>(1.., wsc()).void()
}

/// Matches 1 or more whitespace, but not allow comment.
pub fn ws1<'a, S, E>() -> impl Parser<S, (), E> + 'a
where
    S: Stream<Token = u8, Slice = &'a [u8]> + StreamIsPartial + Compare<u8> + 'a,
    E: ParserError<S> + 'a,
{
    repeat::<_, _, (), _, _>(1.., whitespace()).void()
}

/// Convert a parser to a parser that prefixed with 0 or more whitespace and/or comment.
pub(crate) fn wsc_prefixed0<'a, S, F, O, E>(inner: F) -> impl Parser<S, O, E> + 'a
where
    S: Stream<Token = u8, Slice = &'a [u8]>
        + StreamIsPartial
        + Compare<u8>
        + Compare<&'a [u8]>
        + 'a,
    F: Parser<S, O, E> + 'a,
    O: 'a,
    E: ParserError<S> + 'a,
{
    preceded(wsc0(), inner)
}

/// Convert a parser to a parser that prefixed with 1 or more whitespace.
fn ws_prefixed1<'a, S, F, O, E>(inner: F) -> impl Parser<S, O, E> + 'a
where
    S: Stream<Token = u8, Slice = &'a [u8]> + StreamIsPartial + Compare<u8> + 'a,
    F: Parser<S, O, E> + 'a,
    O: 'a,
    E: ParserError<S> + 'a,
{
    preceded(repeat::<_, _, (), _, _>(1.., whitespace()).void(), inner)
}

/// Convert a parser to a parser that prefixed with 0 or more whitespace.
fn ws_prefixed0<'a, S, F, O, E>(inner: F) -> impl Parser<S, O, E> + 'a
where
    S: Stream<Token = u8, Slice = &'a [u8]> + StreamIsPartial + Compare<u8> + 'a,
    F: Parser<S, O, E> + 'a,
    O: 'a,
    E: ParserError<S> + 'a,
{
    preceded(repeat::<_, _, (), _, _>(.., whitespace()).void(), inner)
}

#[cfg(test)]
mod tests;
