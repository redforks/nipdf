use nom::{branch::alt, bytes::complete::tag, character::complete::i32, combinator::map};

use crate::object::Object;

use super::ParseResult;

pub fn parse_object(buf: &[u8]) -> ParseResult<'_, Object> {
    let null_parser = map(tag("null"), |_| Object::Null);
    let true_parser = map(tag("true"), |_| Object::Bool(true));
    let false_parser = map(tag("false"), |_| Object::Bool(false));
    let i32_parser = map(i32, |v| Object::Integer(v));
    alt((null_parser, true_parser, false_parser, i32_parser))(buf)
}

#[cfg(test)]
mod tests;
