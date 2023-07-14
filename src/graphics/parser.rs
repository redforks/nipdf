use lazy_static::lazy_static;
use std::collections::HashSet;

use nom::{branch::alt, bytes::complete::is_not, combinator::map_res, Parser};

use crate::{
    object::Object,
    parser::{parse_object, ParseError, ParseResult, PdfParseError},
};

#[derive(Debug, PartialEq)]
enum ObjectOrOperator<'a> {
    Object(Object<'a>),
    Operator(&'a str),
}

lazy_static! {
    static ref OPERATORS: HashSet<&'static str> = [
        // General graphics state
        "w", "J", "j", "M", "d", "ri", "i", "gs",
        // Special graphics state
        "q", "Q", "cm",
        // Path construction
        "m", "l", "c", "v", "y", "h", "re",
        // Path Painting
        "S", "s", "f", "F", "f*", "B", "B*", "b", "b*", "n",
        // Clipping paths
        "W", "W*",
        // Text objects
        "BT", "ET",
        // Text state
        "Tc", "Tw", "Tz", "TL", "Tf", "Tr", "Ts",
        // Text positioning
        "Td", "TD", "Tm", "T*",
        // Text showing
        "Tj", "TJ","'", "\"",
        // Type 3 font
        "d0", "d1",
        // Color
        "CS", "cs", "SC", "SCN", "sc", "scn", "G", "g", "RG", "rg", "K", "k",
        // Shading patterns
        "sh",
        // Inline images
        "BI", "ID", "EI",
        // XObjects
        "Do",
        // Marked content
        "MP", "DP", "BMC", "BDC", "EMC",
        // Compatibility
        "BX", "EX",
    ].iter().cloned().collect();
}

fn parse_operator(input: &[u8]) -> ParseResult<ObjectOrOperator> {
    let p = is_not(b" \t\n\r#".as_slice());
    map_res(p, |op| {
        let op = unsafe { std::str::from_utf8_unchecked(op) };
        if OPERATORS.contains(op) {
            Ok(ObjectOrOperator::Operator(op))
        } else {
            Err(ParseError::UnknownGraphicOperator(op.to_owned()))
        }
    })(input)
}

fn parse_object_or_operator(input: &[u8]) -> ParseResult<ObjectOrOperator> {
    alt((
        parse_object.map(|o| ObjectOrOperator::Object(o)),
        parse_operator,
    ))(input)
}

#[cfg(test)]
mod tests;
