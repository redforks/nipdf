use nom::{
    branch::alt,
    bytes::complete::{escaped, is_not, tag, take_until},
    character::complete::{char, hex_digit0, none_of},
    combinator::{map, recognize},
    multi::many0,
    number::complete::float,
    sequence::delimited,
    AsChar, InputTakeAtPosition,
};
use num::cast;

use crate::{object::Object, parser::ParseError};

use super::ParseResult;

pub fn parse_object(buf: &[u8]) -> ParseResult<'_, Object> {
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
        let inner_parser = alt((
            tag(b"\\("),
            tag(b"\\)"),
            is_not(b"()".as_slice()),
            parse_quoted_string,
        ));
        let mut parser = recognize(delimited(tag(b"("), many0(inner_parser), tag(b")")));
        parser(input)
    }
    let parse_quoted_string = map(parse_quoted_string, |s| Object::LiteralString(s));
    let parse_hex_string = map(
        recognize(delimited(tag(b"<"), take_until(b">".as_slice()), tag(b">"))),
        |s| Object::HexString(s),
    );

    alt((
        null,
        true_parser,
        false_parser,
        number_parser,
        parse_quoted_string,
        parse_hex_string,
    ))(buf)
}

#[cfg(test)]
mod tests;
