use crate::{
    ascii85::{self, Ascii85Error},
    machine::{Token, TokenArray, Value},
    name,
    type1::Header,
};
use either::Either;
use snafu::{FromString, Whatever, prelude::*};
use std::{
    cell::RefCell,
    iter::once,
    num::ParseIntError,
    rc::Rc,
    str::{Utf8Error, from_utf8},
    string::FromUtf8Error,
};
use winnow::{
    PResult, Parser,
    ascii::hex_digit1,
    combinator::{alt, delimited, dispatch, fail, opt, preceded, repeat, terminated},
    error::{
        AddContext, ErrMode, ErrorKind, FromExternalError, ParseError, ParserError as _, StrContext,
    },
    stream::{AsChar, Stream},
    token::{any, literal, one_of, take_till, take_while},
};

#[derive(Snafu, Debug)]
pub enum ParserError<C: 'static = StrContext> {
    Leaf {
        kind: ErrorKind,
        context: Option<C>,
    },
    Inter {
        #[snafu(source(from(ParserError<C>, Box::new)))]
        source: Box<ParserError<C>>,
        kind: ErrorKind,
        context: Option<C>,
    },
    StringEncoding {
        source: FromUtf8Error,
        context: Option<C>,
    },
    StrEncoding {
        source: Utf8Error,
        context: Option<C>,
    },
    ParseInt {
        source: ParseIntError,
        context: Option<C>,
    },
    Ascii85 {
        source: Ascii85Error,
        context: Option<C>,
    },
}

impl<C: 'static, I> FromExternalError<I, Utf8Error> for ParserError<C> {
    fn from_external_error(_input: &I, kind: ErrorKind, e: Utf8Error) -> Self {
        Self::Inter {
            source: Box::new(Self::StrEncoding {
                source: e,
                context: None,
            }),
            kind,
            context: None,
        }
    }
}

enum PossibleError {
    Utf8(Utf8Error),
    Utf8Str(FromUtf8Error),
    Int(ParseIntError),
    Ascii85(Ascii85Error),
}

impl From<Utf8Error> for PossibleError {
    fn from(err: Utf8Error) -> Self {
        PossibleError::Utf8(err)
    }
}

impl From<FromUtf8Error> for PossibleError {
    fn from(err: FromUtf8Error) -> Self {
        PossibleError::Utf8Str(err)
    }
}

impl From<ParseIntError> for PossibleError {
    fn from(err: ParseIntError) -> Self {
        PossibleError::Int(err)
    }
}

impl From<Ascii85Error> for PossibleError {
    fn from(value: Ascii85Error) -> Self {
        PossibleError::Ascii85(value)
    }
}

impl<C: 'static, I> FromExternalError<I, PossibleError> for ParserError<C> {
    fn from_external_error(_input: &I, kind: ErrorKind, e: PossibleError) -> Self {
        Self::Inter {
            source: Box::new(match e {
                PossibleError::Utf8(e) => Self::StrEncoding {
                    source: e,
                    context: None,
                },
                PossibleError::Utf8Str(e) => Self::StringEncoding {
                    source: e,
                    context: None,
                },
                PossibleError::Int(e) => Self::ParseInt {
                    source: e,
                    context: None,
                },
                PossibleError::Ascii85(e) => Self::Ascii85 {
                    source: e,
                    context: None,
                },
            }),
            kind,
            context: None,
        }
    }
}

pub(crate) fn perror_to_whatever(err: ErrMode<ParserError>, msg: impl Into<String>) -> Whatever {
    match err.into_inner() {
        Some(err) => Whatever::with_source(Box::new(err), msg.into()),
        None => unreachable!(),
    }
}

pub(crate) fn parse_error_to_whatever<I>(
    err: ParseError<I, ParserError>,
    msg: impl Into<String>,
) -> Whatever {
    let e = err.into_inner();
    Whatever::with_source(Box::new(e), msg.into())
}

impl<I: Stream, C> AddContext<I, C> for ParserError<C> {
    fn add_context(
        mut self,
        _input: &I,
        _token_start: &<I as Stream>::Checkpoint,
        context: C,
    ) -> Self {
        match &mut self {
            Self::Leaf { context: c, .. }
            | Self::Inter { context: c, .. }
            | Self::StringEncoding { context: c, .. }
            | Self::StrEncoding { context: c, .. }
            | Self::ParseInt { context: c, .. }
            | Self::Ascii85 { context: c, .. } => *c = Some(context),
        }
        self
    }
}

impl<I: Stream> winnow::error::ParserError<I> for ParserError {
    fn from_error_kind(_input: &I, kind: ErrorKind) -> Self {
        Self::Leaf {
            kind,
            context: None,
        }
    }

    fn append(self, _input: &I, _token_start: &<I as Stream>::Checkpoint, kind: ErrorKind) -> Self {
        Self::Inter {
            source: Box::new(self),
            kind,
            context: None,
        }
    }
}

/// Parses the header of a Type 1 font. The header is the first line of the
/// file, and is of the form:
///
///    %!PS-AdobeFont-1.0: Times-Roman 001.001
///
/// The first token is the version of the Type 1 specification that the font
/// conforms to. The second token is the font name. The third token is the
/// font version.
pub fn header(input: &mut &[u8]) -> PResult<Header, ParserError> {
    preceded(
        literal(b"%!"),
        alt((b"PS-AdobeFont", b"AdobeFont", b"FontType1")),
    )
    .parse_next(input)?;
    let spec_ver = delimited('-', take_till(1.., ':'), b": ").parse_next(input)?;
    let font_name = take_till(1.., ' ').parse_next(input)?;
    let font_ver = delimited(
        ' ',
        take_while(1.., (('0'..='9'), '.', ('a'..='z'))),
        loose_line_ending,
    )
    .parse_next(input)?;

    Ok(Header {
        spec_ver: String::from_utf8(spec_ver.to_owned())
            .context(StringEncodingSnafu { context: None })
            .map_err(ErrMode::Backtrack)?,
        font_name: String::from_utf8(font_name.to_owned())
            .context(StringEncodingSnafu { context: None })
            .map_err(ErrMode::Backtrack)?,
        font_ver: String::from_utf8(font_ver.to_owned())
            .context(StringEncodingSnafu { context: None })
            .map_err(ErrMode::Backtrack)?,
    })
}

fn comment(input: &mut &[u8]) -> PResult<(), ParserError> {
    preceded(
        literal(b"%"),
        take_till(0.., |c| c == b'\n' || c == b'\r' || c == b'\x0c'),
    )
    .parse_next(input)?;
    Ok(())
}

/// 0x0, 0x9, 0x0A, 0x0C, 0x0D, 0x20
fn is_white_space(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == b'\n' || b == b'\x0C' || b == b'\r' || b == b'\0'
}

fn is_delimiter(b: u8) -> bool {
    b == b'('
        || b == b')'
        || b == b'<'
        || b == b'>'
        || b == b'['
        || b == b']'
        || b == b'{'
        || b == b'}'
        || b == b'/'
        || b == b'%'
}

/// not white space and delimiter
fn is_regular_char(b: u8) -> bool {
    !is_white_space(b) && !is_delimiter(b)
}

/// Parses one or more white space bytes
pub fn white_space<'a>(input: &mut &'a [u8]) -> PResult<&'a [u8], ParserError> {
    take_while(1.., is_white_space).parse_next(input)
}

pub fn white_space_or_comment(input: &mut &[u8]) -> PResult<(), ParserError> {
    alt((white_space.value(()), comment)).parse_next(input)
}

/// Ignore preceded whitespace and/or comments
pub fn ws_prefixed<'a, P, O>(p: P) -> impl Parser<&'a [u8], O, ParserError>
where
    P: Parser<&'a [u8], O, ParserError>,
{
    preceded(repeat::<_, _, (), _, _>(.., white_space_or_comment), p)
}

/// Matches '\n', '\r', '\r\n'
fn loose_line_ending(input: &mut &[u8]) -> PResult<(), ParserError> {
    match input.first() {
        Some(b'\n') => {
            input.next_token();
            Ok(())
        }
        Some(b'\r') => {
            input.next_token();
            if input.first() == Some(&b'\n') {
                input.next_token();
            }
            Ok(())
        }
        _ => fail.parse_next(input),
    }
}

fn int_or_float(input: &mut &[u8]) -> PResult<Either<i32, f32>, ParserError> {
    let buf = (
        one_of(('0'..='9', '+', '-', '.')),
        take_while(0.., ('0'..='9', 'a'..='z', 'A'..='Z', '.', '-', '+', '#')),
    )
        .take()
        .parse_next(input)?;
    if let Some(pos) = memchr::memchr(b'#', buf) {
        let (radix, num) = buf.split_at(pos);
        let radix = from_utf8(radix)
            .context(StrEncodingSnafu { context: None })
            .map_err(ErrMode::Backtrack)?
            .parse::<u32>()
            .map_err(|_| ErrMode::Backtrack(ParserError::from_error_kind(input, ErrorKind::Tag)))?;
        let num = i32::from_str_radix(
            from_utf8(&num[1..])
                .context(StrEncodingSnafu { context: None })
                .map_err(ErrMode::Backtrack)?,
            radix,
        )
        .map_err(|_| ErrMode::Backtrack(ParserError::from_error_kind(input, ErrorKind::Tag)))?;
        return Ok(Either::Left(num));
    }

    if memchr::memchr3(b'.', b'e', b'E', buf).is_some() {
        Ok(Either::Right(
            from_utf8(buf)
                .context(StrEncodingSnafu { context: None })
                .map_err(ErrMode::Backtrack)?
                .parse::<f32>()
                .map_err(|_| {
                    ErrMode::Backtrack(ParserError::from_error_kind(input, ErrorKind::Tag))
                })?,
        ))
    } else {
        Ok(
            match from_utf8(buf)
                .context(StrEncodingSnafu { context: None })
                .map_err(ErrMode::Backtrack)?
                .parse::<i32>()
            {
                Ok(v) => Either::Left(v),
                Err(_) => Either::Right(
                    from_utf8(buf)
                        .context(StrEncodingSnafu { context: None })
                        .map_err(ErrMode::Backtrack)?
                        .parse::<f32>()
                        .map_err(|_| {
                            ErrMode::Backtrack(ParserError::from_error_kind(input, ErrorKind::Tag))
                        })?,
                ),
            },
        )
    }
}

fn string(input: &mut &[u8]) -> PResult<Box<[u8]>, ParserError> {
    enum StringFragment<'a> {
        Literal(&'a [u8]),
        EscapedChar(u8),
        EscapedNewLine,
        Nested(Box<[u8]>),
    }

    fn literal_fragment<'a>(input: &mut &'a [u8]) -> PResult<StringFragment<'a>, ParserError> {
        let buf = take_till(1.., (b'(', b')', b'\\')).parse_next(input)?;
        Ok(StringFragment::Literal(buf))
    }

    fn escaped_char<'a>(input: &mut &'a [u8]) -> PResult<StringFragment<'a>, ParserError> {
        let parse_oct_byte = take_while(1..=3, |c: u8| c.is_oct_digit()).try_map(|buf| {
            Ok::<_, PossibleError>((u16::from_str_radix(from_utf8(buf)?, 8)? & 0xff) as u8)
        });

        let c = preceded(
            literal(b"\\"),
            alt((
                b'n'.value(b'\n'),
                b'r'.value(b'\r'),
                b't'.value(b'\t'),
                b'b'.value(b'\x08'),
                b'f'.value(b'\x0C'),
                b'('.value(b'('),
                b')'.value(b')'),
                parse_oct_byte,
            )),
        )
        .parse_next(input)?;
        Ok(StringFragment::EscapedChar(c))
    }

    fn escaped_newline<'a>(input: &mut &'a [u8]) -> PResult<StringFragment<'a>, ParserError> {
        preceded(literal(b"\\"), loose_line_ending).parse_next(input)?;
        Ok(StringFragment::EscapedNewLine)
    }

    fn build_string(input: &mut &[u8]) -> PResult<Box<[u8]>, ParserError> {
        repeat(0.., fragment)
            .fold(Vec::new, |mut r, frag| {
                match frag {
                    StringFragment::Literal(s) => r.extend_from_slice(s),
                    StringFragment::EscapedChar(c) => r.push(c),
                    StringFragment::EscapedNewLine => (),
                    StringFragment::Nested(s) => {
                        r.extend(once(b'(').chain(s.iter().copied()).chain(once(b')')));
                    }
                }
                r
            })
            .parse_next(input)
            .map(Into::into)
    }

    fn nested<'a>(input: &mut &'a [u8]) -> PResult<StringFragment<'a>, ParserError> {
        let frag = delimited(b'(', opt(build_string), b')').parse_next(input)?;
        Ok(StringFragment::Nested(match frag {
            Some(s) => s,
            None => (*b"").into(),
        }))
    }

    fn fragment<'a>(input: &mut &'a [u8]) -> PResult<StringFragment<'a>, ParserError> {
        alt((literal_fragment, escaped_char, escaped_newline, nested)).parse_next(input)
    }

    fn literal_string(input: &mut &[u8]) -> PResult<Box<[u8]>, ParserError> {
        terminated(build_string, b')').parse_next(input)
    }

    /// String encoded in hex wrapped in "<>", e.g. <0123456789ABCDEF>
    /// White space are ignored, if last byte is missing, it is assumed to be 0.
    fn hex_string(input: &mut &[u8]) -> PResult<Box<[u8]>, ParserError> {
        let bytes = repeat(0.., alt((hex_digit1, white_space)))
            .fold(Vec::new, |mut bytes, frag| {
                if !is_white_space(frag[0]) {
                    bytes.extend(frag);
                }
                bytes
            })
            .try_map(|mut s| {
                if s.len() % 2 != 0 {
                    s.push(b'0');
                }

                let mut bytes = Vec::with_capacity(s.len() / 2);
                for i in (0..s.len()).step_by(2) {
                    bytes.push(u8::from_str_radix(from_utf8(&s[i..i + 2])?, 16)?);
                }
                Ok::<_, PossibleError>(Box::<[u8]>::from(bytes))
            });

        terminated(bytes, b'>').parse_next(input)
    }

    fn ascii85(input: &mut &[u8]) -> PResult<Box<[u8]>, ParserError> {
        delimited(
            b'~',
            take_while(0.., |c| c != b'~')
                .try_map(|v: &[u8]| Ok::<_, PossibleError>(ascii85::decode(from_utf8(v)?)?.into())),
            b"~>",
        )
        .parse_next(input)
    }

    fn hex_or_85(input: &mut &[u8]) -> PResult<Box<[u8]>, ParserError> {
        alt((hex_string, ascii85)).parse_next(input)
    }

    dispatch!(any;
        b'(' => literal_string,
        b'<' => hex_or_85,
        _ => fail,
    )
    .parse_next(input)
}

fn executable_name<'a>(input: &mut &'a [u8]) -> PResult<&'a str, ParserError> {
    take_while(1.., is_regular_char)
        .try_map(from_utf8)
        .parse_next(input)
}

fn literal_name<'a>(input: &mut &'a [u8]) -> PResult<&'a str, ParserError> {
    preceded('/', take_while(0.., is_regular_char).try_map(from_utf8)).parse_next(input)
}

fn procedure(input: &mut &[u8]) -> PResult<TokenArray, ParserError> {
    delimited(b'{', repeat(0.., ws_prefixed(token)), ws_prefixed(b'}')).parse_next(input)
}

/// Parses '[', ']', '<<', '>>' and convert them to String.
fn special_name<'a>(input: &mut &'a [u8]) -> PResult<&'a str, ParserError> {
    let buf = take_while(1..=2, (b'[', ']', b"<<", b">>")).parse_next(input)?;
    from_utf8(buf)
        .context(StrEncodingSnafu { context: None })
        .map_err(ErrMode::Backtrack)
}

pub fn token(input: &mut &[u8]) -> PResult<Token, ParserError> {
    alt((
        int_or_float.map(|v| Token::Literal(v.either(Value::Integer, Value::Real))),
        string.map(|s| Token::Literal(Vec::from(s).into())),
        literal_name.map(|s| Token::Literal(Value::Name(name(s)))),
        special_name.map(|s| Token::Name(name(s))),
        procedure.map(|a| Token::Literal(Value::Procedure(Rc::new(RefCell::new(a))))),
        executable_name.map(|s| Token::Name(name(s))),
    ))
    .parse_next(input)
}

#[cfg(test)]
mod tests;
