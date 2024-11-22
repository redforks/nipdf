use super::eol_now;
use crate::{
    ParserError,
    object::{HexString, InnerString, LiteralString, Object, ObjectValueError},
    parser::is_whitespace,
};
use hex::FromHexError;
use log::warn;
use nom::AsBytes;
use prescript::Name;
use snafu::{OptionExt, ResultExt as _, Whatever};
use std::{borrow::Cow, str::from_utf8};
use winnow::{
    PResult, Parser,
    ascii::float,
    combinator::{alt, delimited, preceded, repeat, rest},
    stream::{AsChar, ContainsToken},
    token::{any, take_till, take_while},
};

fn name<'a>() -> impl Parser<&'a [u8], Name, ParserError> {
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

fn number<'a>() -> impl Parser<&'a [u8], Object, ParserError> {
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

#[derive(Clone)]
enum LiteralStringFragment<'a> {
    Literal(&'a [u8]),
    Escaped(u8),
    EscapedLine,
    Nested(LiteralString),
}

fn parse_quoted_string(input: &mut &[u8]) -> PResult<LiteralString, ParserError> {
    let literal = take_till(1.., b"\\()").map(LiteralStringFragment::Literal);
    let paired = parse_quoted_string.map(LiteralStringFragment::Nested);
    let oct_char = take_while(1..4, AsChar::is_oct_digit)
        .try_map(|s: &[u8]| {
            u8::from_str_radix(from_utf8(s).whatever_context::<_, Whatever>("not utf8")?, 8)
                .whatever_context::<_, Whatever>("parse oct")
        })
        .map(LiteralStringFragment::Escaped);
    let escaped_line = eol_now().value(LiteralStringFragment::EscapedLine);
    let escaped = preceded(
        b'\\',
        alt((
            'n'.value(LiteralStringFragment::Escaped(b'\n')),
            'r'.value(LiteralStringFragment::Escaped(b'\r')),
            't'.value(LiteralStringFragment::Escaped(b'\t')),
            'b'.value(LiteralStringFragment::Escaped(b'\x08')),
            'f'.value(LiteralStringFragment::Escaped(b'\x0C')),
            oct_char,
            escaped_line,
            any.map(LiteralStringFragment::Escaped),
        )),
    );
    // .map(LiteralStringFragment::Literal);
    delimited(
        b'(',
        repeat(0.., alt((literal, paired, escaped))).fold(InnerString::new, |mut r, f| {
            match f {
                LiteralStringFragment::Literal(s) => r.extend_from_slice(s),
                LiteralStringFragment::Escaped(c) => r.push(c),
                LiteralStringFragment::Nested(mut s) => {
                    r.push(b'(');
                    r.append(&mut s.0);
                    r.push(b')');
                }
                LiteralStringFragment::EscapedLine => {}
            }
            r
        }),
        b')',
    )
    .map(LiteralString)
    .parse_next(input)
}

fn decode_hex(buf: &[u8]) -> Result<HexString, FromHexError> {
    /// Remove whitespace from hex string.
    fn preprocess(buf: &[u8]) -> Cow<'_, [u8]> {
        let is_ws = is_whitespace();
        if (buf.len() % 2 == 0) && !buf.iter().any(|&c| is_ws.contains_token(c)) {
            return buf.into();
        }

        let mut result: Vec<u8> = buf
            .iter()
            .copied()
            .filter(|&c| !is_ws.contains_token(c))
            .collect();
        if result.len() % 2 != 0 {
            result.push(b'0');
        }
        result.into()
    }

    let buf = preprocess(buf);
    hex::decode(&buf).map(|v| HexString(v.as_bytes().into()))
}

fn hex_string<'a>() -> impl Parser<&'a [u8], Object, ParserError> {
    let parser = take_while(
        ..,
        (AsChar::is_hex_digit, [b' ', b'\t', b'\r', b'\n', b'\x0C']),
    );
    delimited(b'<', parser.try_map(decode_hex), b'>').map(Object::HexString)
}

/// Return parser to parse [Object].
fn object<'a>() -> impl Parser<&'a [u8], Object, ParserError> {
    let null = b"null".value(Object::Null);
    let bool = alt((
        b"true".value(Object::Bool(true)),
        b"false".value(Object::Bool(false)),
    ));
    let name = name().map(Object::Name);
    let quoted_string = parse_quoted_string.map(Object::LiteralString);

    alt((null, bool, number(), name, quoted_string, hex_string()))
}

#[cfg(test)]
mod tests;
