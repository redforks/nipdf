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
    Parser,
    combinator::{alt, preceded},
    token::{take_till, take_while},
};

fn parse_number(s: &[u8]) -> Result<Object, Whatever> {
    fn parse_int(s: &str) -> Result<Object, ParseIntError> {
        s.parse().map(Object::Integer)
    }

    fn parse_f32(s: &str) -> Result<Object, ParseFloatError> {
        s.parse().map(Object::Number)
    }

    if memchr::memchr(b'.', s).is_some() {
        let s = from_utf8(s).whatever_context("convert utf8")?;
        parse_f32(s).or_else(|e| {
            let s = s.as_bytes();
            let p = memchr::memchr(b'.', s).whatever_context("failed to find '.'")?;
            // get position of 2nd occur of '.'
            if let Some(p) = memchr::memchr(b'.', &s[p + 1..]) {
                // if there is a 2nd occur of '.', ignore it
                let s = from_utf8(&s[..p + 1]).whatever_context("convert utf8")?;
                return Ok(parse_f32(s).whatever_context("convert f32")?);
            }
            Err(Whatever::with_source(
                Box::new(e),
                "parse f32 failed".to_owned(),
            ))
        })
    } else {
        let s = from_utf8(s).whatever_context("convert utf8")?;
        parse_int(s)
            .or_else(|_| parse_f32(s))
            .or_else(|e| {
                if s == "-" {
                    Ok(Object::Number(0.))
                } else {
                    Err(e)
                }
            })
            .with_whatever_context(|_| format!("parse number from: '{}'", s))
    }
}

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

fn object_parser<'a>() -> impl Parser<&'a [u8], Object, ParserError> {
    let null = b"null".value(Object::Null);
    let bool = alt((
        b"true".value(Object::Bool(true)),
        b"false".value(Object::Bool(false)),
    ));
    let number = take_while(1.., (b'0'..=b'9', b'+', b'-', b'.'))
        .take()
        .try_map(parse_number);
    let name_parser = name_parser().map(Object::Name);

    alt((null, bool, number, name_parser))
}

#[cfg(test)]
mod tests;
