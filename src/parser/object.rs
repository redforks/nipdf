use nom::{branch::alt, bytes::complete::tag, combinator::map, number::complete::float};
use num::cast;

use crate::object::Object;

use super::ParseResult;

pub fn parse_object(buf: &[u8]) -> ParseResult<'_, Object> {
    let null_parser = map(tag("null"), |_| Object::Null);
    let true_parser = map(tag("true"), |_| Object::Bool(true));
    let false_parser = map(tag("false"), |_| Object::Bool(false));
    let f32_parser = map(float, |v| {
        if let Some(i) = cast(v) {
            Object::Integer(i)
        } else {
            Object::Number(v)
        }
    });
    alt((null_parser, true_parser, false_parser, f32_parser))(buf)
}

#[cfg(test)]
mod tests;
