use std::borrow::Cow;

use nom::{
    branch::alt,
    bytes::{
        complete::{escaped, is_not, tag, take_till, take_until, take_while},
        streaming::take,
    },
    character::{
        complete::{anychar, crlf, multispace1, u16, u32},
        is_hex_digit,
    },
    combinator::{map, not, opt, peek, recognize, value},
    multi::{many0, many0_count},
    number::complete::float,
    sequence::{delimited, preceded, separated_pair, terminated, tuple},
};
use num::cast;

use crate::object::{
    Array, Dictionary, HexString, IndirectObject, LiteralString, Name, Object, ObjectValueError,
    Reference, Stream,
};

use super::{whitespace_or_comment, ws, ws_prefixed, ws_terminated, ParseError, ParseResult};

/// Unwrap the result of nom parser to a *normal* result.
pub fn unwrap_parse_result<'a, T: 'a>(obj: ParseResult<'a, T>) -> Result<T, ParseError<'a>> {
    match obj {
        Ok((_, obj)) => Ok(obj),
        Err(nom::Err::Incomplete(_)) => unreachable!(),
        Err(nom::Err::Error(e)) | Err(nom::Err::Failure(e)) => Err(e),
    }
}

pub fn parse_object(buf: &[u8]) -> ParseResult<Object> {
    let null = value(Object::Null, tag(b"null"));
    let true_parser = value(Object::Bool(true), tag(b"true"));
    let false_parser = value(Object::Bool(false), tag(b"false"));
    let number_parser = map(float, |v| {
        if let Some(i) = cast(v) {
            if v.fract() == 0.0 {
                Object::Integer(i)
            } else {
                Object::Number(v)
            }
        } else {
            Object::Number(v)
        }
    });

    fn parse_quoted_string(input: &[u8]) -> ParseResult<&[u8]> {
        let esc = escaped(is_not("\\()"), '\\', anychar);
        let inner_parser = alt((esc, parse_quoted_string));
        let mut parser = recognize(delimited(tag(b"("), many0_count(inner_parser), tag(b")")));
        parser(input)
    }
    let parse_quoted_string = map(parse_quoted_string, |s| {
        Object::LiteralString(LiteralString::new(s))
    });
    let parse_hex_string = map(
        recognize(delimited(
            tag(b"<"),
            take_while(|c| is_hex_digit(c) || c.is_ascii_whitespace()),
            tag(b">"),
        )),
        |buf| Object::HexString(HexString::new(buf)),
    );

    alt((
        map(parse_name, Object::Name),
        parse_quoted_string,
        map(parse_dict, Object::Dictionary),
        map(parse_array, Object::Array),
        parse_hex_string,
        null,
        true_parser,
        false_parser,
        map(parse_reference, Object::Reference),
        number_parser,
    ))(buf)
}

/// Return `Err(ObjectValueError::InvalidNameForma)` if the name is not a valid PDF name encoding,
/// not two hex char after `#`.
fn normalize_name(buf: &[u8]) -> Result<Cow<[u8]>, ObjectValueError> {
    fn next_hex_char(iter: &mut impl Iterator<Item = u8>) -> Option<u8> {
        let mut result = 0;
        for _ in 0..2 {
            if let Some(c) = iter.next() {
                result <<= 4;
                result |= match c {
                    b'0'..=b'9' => c - b'0',
                    b'a'..=b'f' => c - b'a' + 10,
                    b'A'..=b'F' => c - b'A' + 10,
                    _ => return None,
                };
            } else {
                return None;
            }
        }
        Some(result)
    }

    let s = &buf[1..];
    if s.iter().copied().any(|b| b == b'#') {
        let mut result = Vec::with_capacity(s.len());
        let mut iter = s.iter().copied();
        while let Some(next) = iter.next() {
            if next == b'#' {
                if let Some(c) = next_hex_char(&mut iter) {
                    result.push(c);
                } else {
                    return Err(ObjectValueError::InvalidNameFormat);
                }
            } else {
                result.push(next);
            }
        }
        Ok(Cow::Owned(result))
    } else {
        Ok(Cow::Borrowed(s))
    }
}

fn parse_name(input: &[u8]) -> ParseResult<Name> {
    let (input, buf) = recognize(preceded(
        tag(b"/"),
        take_till(|c: u8| {
            c.is_ascii_whitespace()
                || c == b'['
                || c == b'<'
                || c == b'('
                || c == b'/'
                || c == b'>'
                || c == b']'
        }),
    ))(input)?;
    let name = normalize_name(buf)
        .map_err(|_| nom::Err::Error(ParseError::InvalidNameFormat))
        .map(Name)?;
    Ok((input, name))
}

pub fn parse_array(input: &[u8]) -> ParseResult<Array> {
    delimited(
        ws(tag(b"[")),
        many0(ws(parse_object)),
        ws_terminated(tag(b"]")),
    )(input)
}

pub fn parse_dict(input: &[u8]) -> ParseResult<Dictionary> {
    map(
        delimited(
            ws(tag(b"<<".as_slice())),
            many0(tuple((parse_name, ws(parse_object)))),
            ws_terminated(tag(b">>")),
        ),
        |v| v.into_iter().collect(),
    )(input)
}

pub fn parse_stream_content(input: &[u8], stream_len: u32) -> ParseResult<&[u8]> {
    delimited(
        tuple((ws_prefixed(tag(b"stream")), alt((crlf, tag(b"\n"))))),
        take(stream_len),
        ws(tag(b"endstream")),
    )(input)
}

fn parse_object_and_stream(input: &[u8]) -> ParseResult<Object> {
    let (input, o) = parse_object(input)?;
    let (input, buf) = match o {
        Object::Dictionary(ref d) => {
            let Some(len) = d.get(&Name::borrowed(b"Length")) else {
                return Ok((input, o));
            };
            match len {
                Object::Integer(len) => opt(delimited(
                    tuple((ws_prefixed(tag(b"stream")), alt((crlf, tag(b"\n"))))),
                    take(*len as u32),
                    ws(tag(b"endstream")),
                ))(input)?,
                Object::Reference(_) => {
                    let (input, inner) = peek(opt(ws_prefixed(tag(b"stream"))))(input)?;
                    return match inner {
                        None => Ok((input, o)),
                        Some(_) => {
                            let (input, _) =
                                take_until::<&[u8], &[u8], ParseError>(b"endobj")(input)?;
                            Ok((input, Object::LaterResolveStream(d.clone())))
                        }
                    };
                }
                _ => return Ok((input, o)),
            }
        }
        _ => return Ok((input, o)),
    };
    match buf {
        Some(buf) => match o {
            Object::Dictionary(d) => Ok((input, Object::Stream(Stream(d, buf)))),
            _ => unreachable!(),
        },
        None => Ok((input, o)),
    }
}

pub fn parse_indirected_object(input: &[u8]) -> ParseResult<IndirectObject> {
    let (input, (id, gen)) = separated_pair(u32, multispace1, u16)(input)?;
    let (input, obj) =
        delimited(ws(tag(b"obj")), parse_object_and_stream, ws(tag(b"endobj")))(input)?;
    Ok((input, IndirectObject::new(id, gen, obj)))
}

fn parse_reference(input: &[u8]) -> ParseResult<Reference> {
    let (input, (id, gen)) = terminated(
        separated_pair(u32, multispace1, u16),
        // `not(peek(tag("G")))` to detect `RG` graphics operation,
        // such as `0 1 0 RG` is a valid graphics operation,
        // if not check `not(peek(tag("G")))`, the sequence will parsed to
        // Integer(0), Reference(1 0)
        delimited(whitespace_or_comment, tag(b"R"), not(peek(tag("G")))),
    )(input)?;
    Ok((input, Reference::new(id, gen)))
}

#[cfg(test)]
mod tests;
