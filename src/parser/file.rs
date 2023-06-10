use nom::{
    bytes::complete::tag,
    character::complete::{char, line_ending, satisfy},
    combinator::{map_res, recognize},
    sequence::{preceded, terminated, tuple},
};

use crate::file::Header;

use super::{LogicParseError, ParseResult};

fn parse_header(buf: &[u8]) -> ParseResult<'_, Header<'_>> {
    let one_digit = || satisfy(|c| c.is_digit(10));

    fn new_header(buf: &[u8]) -> std::result::Result<Header<'_>, LogicParseError> {
        assert_eq!(3, buf.len());
        if buf[0] != b'1' {
            Err(LogicParseError::UnsupportedVersion(
                String::from_utf8_lossy(buf).to_string(),
            ))
        } else {
            Ok(Header::new(buf))
        }
    }

    terminated(
        preceded(
            tag("%PDF-"),
            map_res(
                recognize(tuple((one_digit(), char('.'), one_digit()))),
                new_header,
            ),
        ),
        line_ending,
    )(buf)
}

#[cfg(test)]
mod tests;
