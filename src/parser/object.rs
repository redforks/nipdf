use nom::{
    branch::alt,
    bytes::{
        complete::{escaped, is_not, tag, take_till, take_while},
        streaming::take,
    },
    character::{
        complete::{anychar, char, crlf, multispace0, multispace1, one_of, u16, u32},
        is_hex_digit,
    },
    combinator::{consumed, map, recognize},
    multi::{many0, many0_count},
    number::complete::float,
    sequence::{delimited, preceded, separated_pair, terminated, tuple},
};
use num::cast;

use crate::object::{Array, Dictionary, IndirectObject, Name, Object, Reference, Stream};

use super::{ws, ws_prefixed, ws_terminated, ParseError, ParseResult};

/// Unwrap the result of nom parser to a *normal* result.
pub fn unwrap_parse_result<'a, T: 'a>(obj: ParseResult<'a, T>) -> Result<T, ParseError<'a>> {
    match obj {
        Ok((_, obj)) => Ok(obj),
        Err(nom::Err::Incomplete(_)) => unreachable!(),
        Err(nom::Err::Error(e)) | Err(nom::Err::Failure(e)) => Err(e),
    }
}

pub fn parse_object(buf: &[u8]) -> ParseResult<'_, Object<'_>> {
    let null = map(tag("null"), |_| Object::Null);
    let true_parser = map(tag("true"), |_| Object::Bool(true));
    let false_parser = map(tag("false"), |_| Object::Bool(false));
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

    fn parse_quoted_string(input: &[u8]) -> ParseResult<'_, &[u8]> {
        let esc = escaped(is_not("\\()"), '\\', anychar);
        let inner_parser = alt((esc, parse_quoted_string));
        let mut parser = recognize(delimited(tag(b"("), many0_count(inner_parser), tag(b")")));
        parser(input)
    }
    let parse_quoted_string = map(parse_quoted_string, Object::LiteralString);
    let parse_hex_string = map(
        recognize(delimited(
            tag(b"<"),
            take_while(|c| is_hex_digit(c) || c.is_ascii_whitespace()),
            tag(b">"),
        )),
        Object::HexString,
    );

    alt((
        null,
        true_parser,
        false_parser,
        map(parse_reference, Object::Reference),
        number_parser,
        parse_quoted_string,
        parse_hex_string,
        map(parse_array, Object::Array),
        map(parse_name, Object::Name),
        map(parse_stream, Object::Stream),
        map(parse_dict, Object::Dictionary),
    ))(buf)
}

fn parse_name(input: &[u8]) -> ParseResult<'_, Name<'_>> {
    map(
        recognize(preceded(
            tag(b"/".as_slice()),
            take_till(|c: u8| {
                c.is_ascii_whitespace()
                    || c == b'['
                    || c == b'<'
                    || c == b'('
                    || c == b'/'
                    || c == b'>'
                    || c == b']'
            }),
        )),
        Name::new,
    )(input)
}

pub fn parse_array(input: &[u8]) -> ParseResult<'_, Array<'_>> {
    delimited(
        ws(tag(b"[")),
        many0(ws(parse_object)),
        ws_terminated(tag(b"]")),
    )(input)
}

pub fn parse_dict(input: &[u8]) -> ParseResult<'_, Dictionary<'_>> {
    map(
        delimited(
            ws(tag(b"<<".as_slice())),
            many0(tuple((ws_prefixed(parse_name), ws(parse_object)))),
            ws_terminated(tag(b">>")),
        ),
        |v| v.into_iter().collect(),
    )(input)
}

pub fn parse_stream(input: &[u8]) -> ParseResult<'_, Stream<'_>> {
    let (input, dict) = ws_prefixed(parse_dict)(input)?;
    let len = dict
        .get(&Name::new(b"/Length"))
        .ok_or(nom::Err::Error(ParseError::StreamRequireLength))?;
    let len = len
        .as_int()
        .map_err(|_| nom::Err::Error(ParseError::StreamInvalidLengthType))?;
    let (input, buf) = delimited(
        delimited(multispace0, tag(b"stream"), alt((crlf, tag(b"\n")))),
        take(len as u32),
        ws_prefixed(tag(b"endstream")),
    )(input)?;
    Ok((input, (dict, buf)))
}

pub fn parse_indirected_object(input: &[u8]) -> ParseResult<'_, IndirectObject> {
    let (input, (id, gen)) = separated_pair(u32, multispace1, u16)(input)?;
    let (input, obj) = delimited(ws(tag(b"obj")), parse_object, ws(tag(b"endobj")))(input)?;
    Ok((input, IndirectObject::new(id, gen, obj)))
}

fn parse_reference(input: &[u8]) -> ParseResult<'_, Reference> {
    let (input, (id, gen)) = terminated(
        separated_pair(u32, multispace1, u16),
        ws_prefixed(tag(b"R")),
    )(input)?;
    Ok((input, Reference::new(id, gen)))
}

#[cfg(test)]
mod tests;
