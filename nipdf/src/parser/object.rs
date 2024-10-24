use super::{ParseError, ParseResult, whitespace_or_comment, ws, ws_prefixed, ws_terminated};
use crate::object::{
    Array, BufPos, Dictionary, HexString, IndirectObject, LiteralString, Object, ObjectId,
    ObjectValueError, Reference, Stream,
};
use either::Either;
use log::warn;
use nom::{
    branch::alt,
    bytes::complete::{escaped, is_not, tag, take_till, take_while, take_while1},
    character::{
        complete::{anychar, line_ending, multispace1, u16, u32},
        is_digit, is_hex_digit,
    },
    combinator::{map, not, opt, peek, recognize, value},
    error::{ErrorKind, FromExternalError, ParseError as ParseErrorTrait},
    multi::{many0, many0_count},
    sequence::{delimited, preceded, separated_pair, terminated, tuple},
};
use num_traits::ToPrimitive;
use prescript::{Name, sname};
use std::{
    borrow::Cow,
    num::NonZeroU32,
    rc::Rc,
    str::{FromStr, from_utf8, from_utf8_unchecked},
};

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
    let number_parser = map(
        take_while1(|c| is_digit(c) || c == b'.' || c == b'+' || c == b'-'),
        |s| {
            if memchr::memchr(b'.', s).is_some() {
                // from_utf8_unchecked is safe here, because the parser takes only digits
                let s = unsafe { from_utf8_unchecked(s) };
                f32::from_str(s).map_or_else(
                    |e| {
                        // get position of 2nd occur of '.'
                        let s = s.as_bytes();
                        let p = memchr::memchr(b'.', s).unwrap();
                        if let Some(p) = memchr::memchr(b'.', &s[p + 1..]) {
                            // if there is a 2nd occur of '.', ignore it
                            Object::Number(
                                f32::from_str(unsafe { from_utf8_unchecked(&s[..p + 1]) }).unwrap(),
                            )
                        } else {
                            panic!("{}", e);
                        }
                    },
                    Object::Number,
                )
            } else {
                // from_utf8_unchecked is safe here, because the parser takes only digits
                let s = unsafe { from_utf8_unchecked(s) };
                i32::from_str(s)
                    .map(Object::Integer)
                    .unwrap_or_else(|_| Object::Number(f32::from_str(s).unwrap_or_default()))
            }
        },
    );

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

/// Return `Err(ObjectValueError::InvalidNameFormat)` if the name is not a valid PDF name encoding,
/// not two hex char after `#`.
fn normalize_name(buf: &[u8]) -> Result<Cow<str>, ObjectValueError> {
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
    if s.iter().copied().all(|b| b != b'#') {
        return Ok(Cow::Borrowed(from_utf8(s).unwrap()));
    }

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
    String::from_utf8(result).map_or_else(
        |_| {
            warn!("Invalid UTF-8 name: {:?}", s);
            Ok(Cow::Borrowed(from_utf8(s).unwrap()))
        },
        |s| Ok(Cow::Owned(s)),
    )
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
        .map_err(|e| nom::Err::Error(ParseError::from_external_error(input, ErrorKind::Fail, e)))
        .map(|s| prescript::name(&s))?;
    Ok((input, name))
}

pub fn parse_array(input: &[u8]) -> ParseResult<Array> {
    let (input, arr) = delimited(
        ws(tag(b"[")),
        many0(ws(parse_object)),
        ws_terminated(tag(b"]")),
    )(input)?;

    Ok((input, arr.into()))
}

pub fn parse_dict_entries(input: &[u8]) -> ParseResult<Vec<(Name, Object)>> {
    many0(tuple((parse_name, ws(parse_object))))(input)
}

pub fn parse_dict(input: &[u8]) -> ParseResult<Dictionary> {
    map(
        delimited(
            ws(tag(b"<<".as_slice())),
            parse_dict_entries,
            ws_terminated(tag(b">>")),
        ),
        |v| v.into_iter().collect(),
    )(input)
}

type StreamParts = (Dictionary, u32, Option<NonZeroU32>);

fn parse_object_and_stream(input: &[u8]) -> ParseResult<Either<Object, StreamParts>> {
    let input_len = input.len();
    let (data, o) = parse_object(input)?;
    match o {
        Object::Dictionary(d) => {
            let (mut data, begin_stream) = opt(delimited(
                whitespace_or_comment,
                tag(b"stream"),
                line_ending,
            ))(data)?;
            if begin_stream.is_some() {
                let start = input_len - data.len();
                let length = match d.get(&sname("Length")) {
                    Some(Object::Integer(l)) => Some(*l as u32),
                    _ => None,
                };
                if let Some(length) = length {
                    data = &data[length as usize..];
                    let end_of_line = alt((line_ending, tag(b"\r")));
                    (data, _) = opt(end_of_line)(data)?;
                    (data, _) = tag(b"endstream")(data)?;
                }
                Ok((
                    data,
                    Either::Right((
                        d,
                        start.try_into().unwrap(),
                        length.and_then(NonZeroU32::new),
                    )),
                ))
            } else {
                Ok((data, Either::Left(Object::Dictionary(d))))
            }
        }
        _ => Ok((data, Either::Left(o))),
    }
}

pub fn parse_indirect_object(input: &[u8]) -> ParseResult<'_, IndirectObject> {
    let input_len = input.len();
    let (input, (id, gen)) = ws_prefixed(separated_pair(u32, multispace1, u16))(input)?;
    let (input, _) = ws(tag(b"obj"))(input)?;
    let offset = input_len - input.len();
    let (input, obj) = parse_object_and_stream(input)?;
    let obj = match obj {
        Either::Left(o) => o,
        Either::Right((dict, start, length)) => Object::Stream(Rc::new(Stream::new(
            dict,
            BufPos::new(offset.to_u32().unwrap() + start, length),
            ObjectId::new(id, gen),
        ))),
    };
    let (input, _) = opt(ws_prefixed(tag("endobj")))(input)?;
    Ok((input, IndirectObject::new(id.into(), gen, obj)))
}

/// Parse stream wrapped in indirect object tag,
/// different from `parse_indirect_object()`, buf will after the end of `endobj`
pub fn parse_indirect_stream(input: &[u8]) -> ParseResult<Stream> {
    let (input, o) = parse_indirect_object(input)?;
    let Object::Stream(s) = o.take() else {
        return Err(nom::Err::Failure(ParseError::from_error_kind(
            input,
            ErrorKind::Fail,
        )));
    };
    Ok((input, Rc::into_inner(s).unwrap()))
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
