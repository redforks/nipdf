use nom::{
    branch::alt,
    bytes::complete::{is_not, tag},
    character::complete::{char, none_of},
    combinator::{map, recognize},
    multi::many0,
    number::complete::float,
    sequence::delimited,
};
use num::cast;

use crate::object::Object;

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
            is_not(b"()".as_slice()),
            delimited(tag(b"("), parse_quoted_string, tag(b")")),
        ));
        let mut parser = delimited(tag(b"("), recognize(many0(inner_parser)), tag(b")"));
        parser(input)
    }
    let parse_quoted_string = map(parse_quoted_string, |s| Object::String(s));

    alt((
        null,
        true_parser,
        false_parser,
        number_parser,
        parse_quoted_string,
    ))(buf)
}

#[cfg(test)]
mod tests;
