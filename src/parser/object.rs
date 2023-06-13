use nom::{
    branch::alt,
    bytes::complete::{is_not, tag, take_till, take_until},
    combinator::{map, recognize},
    multi::many0_count,
    number::complete::float,
    sequence::{delimited, preceded},
};
use num::cast;

use crate::object::{Name, Object};

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
        let mut parser = recognize(delimited(tag(b"("), many0_count(inner_parser), tag(b")")));
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
        map(parse_name, Object::Name),
    ))(buf)
}

fn parse_name(input: &[u8]) -> ParseResult<'_, Name<'_>> {
    map(
        recognize(preceded(
            tag(b"/".as_slice()),
            take_till(|c: u8| c.is_ascii_whitespace()),
        )),
        |s| Name::new(s),
    )(input)
}

#[cfg(test)]
mod tests;
