use crate::{
    ParserError,
    object::{Object, ObjectValueError},
};
use log::warn;
use prescript::Name;
use snafu::{FromString, OptionExt, ResultExt as _, Whatever};
use std::{
    borrow::Cow,
    num::{ParseFloatError, ParseIntError},
    str::from_utf8,
};
use winnow::{
    PResult, Parser,
    ascii::float,
    combinator::{alt, preceded, rest},
    token::{take_till, take_while},
};

fn name_parser<'a>() -> impl Parser<&'a [u8], Name, ParserError> {
    preceded(
        b'/',
        take_till(0.., &[
            b' ', b'\t', b'\r', b'\n', b'\x0C', b'[', b'<', b'(', b'/', b'>', b']',
        ]),
    )
    .try_map(normalize_name)
}

/// Return `Err(ObjectValueError::InvalidNameFormat)` if the name is not a valid PDF name encoding,
/// not two hex char after `#`.
fn normalize_name(buf: &[u8]) -> Result<Name, ObjectValueError> {
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

    if !buf.contains(&b'#') {
        return Ok(prescript::name(&String::from_utf8_lossy(buf)));
    }

    let mut result = Vec::with_capacity(buf.len());
    let mut iter = buf.iter().copied();
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
    Ok(prescript::name(&String::from_utf8_lossy(&result)))
}

fn number_parser<'a>() -> impl Parser<&'a [u8], Object, ParserError> {
    let int = rest.parse_to::<i32>().map(Object::Integer);
    let real = rest.parse_to::<f32>().map(Object::Number);
    fn fallback(buf: &mut &[u8]) -> PResult<Object, ParserError> {
        *buf = &[];
        warn!(
            "Invalid number: {},  fallback to 0",
            String::from_utf8_lossy(buf)
        );
        Ok(Object::Integer(0))
    }
    take_while(1.., (b'0'..=b'9', b'+', b'-', b'.'))
        .take()
        .and_then(alt((int, real, float.map(Object::Number), fallback)))
}

fn object_parser<'a>() -> impl Parser<&'a [u8], Object, ParserError> {
    let null = b"null".value(Object::Null);
    let bool = alt((
        b"true".value(Object::Bool(true)),
        b"false".value(Object::Bool(false)),
    ));
    let name_parser = name_parser().map(Object::Name);

    alt((null, bool, number_parser(), name_parser))
}

#[cfg(test)]
mod tests;
