use nom::{
    branch::alt,
    bytes::complete::{is_not, tag, take_till, take_until},
    character::complete::{multispace0, multispace1},
    combinator::{map, recognize},
    multi::{many0, many0_count, separated_list0},
    number::complete::float,
    sequence::{delimited, preceded},
    Parser,
};
use num::cast;

use crate::object::{Array, Name, Object};

use super::{ParseError, ParseResult};

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
        map(parse_array, Object::Array),
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

fn parse_array(input: &[u8]) -> ParseResult<'_, Array<'_>> {
    delimited(
        tag(b"[".as_slice()),
        ws(separated_list0(multispace1, parse_object)),
        tag(b"]".as_slice()),
    )(input)
}

/// A combinator that takes a parser `inner` and produces a parser that also consumes both leading and
/// trailing whitespace, returning the output of `inner`.
fn ws<'a, F, O>(inner: F) -> impl FnMut(&'a [u8]) -> ParseResult<'_, O>
where
    F: Parser<&'a [u8], O, ParseError<'a>>,
{
    delimited(multispace0, inner, multispace0)
}

#[cfg(test)]
mod tests;
