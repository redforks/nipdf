use nom::{
    bytes::complete::tag,
    character::complete::u32,
    character::complete::{char, line_ending, satisfy},
    combinator::{complete, map, map_res, recognize},
    sequence::{preceded, terminated, tuple},
};

use crate::file::{Header, Tail};

use super::{FileError, ParseError, ParseResult};

fn parse_header(buf: &[u8]) -> ParseResult<'_, Header<'_>> {
    let one_digit = || satisfy(|c| c.is_digit(10));

    fn new_header(buf: &[u8]) -> std::result::Result<Header<'_>, FileError> {
        assert_eq!(3, buf.len());
        if buf[0] != b'1' {
            Err(FileError::UnsupportedVersion(
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

/// Reverse iterator of lines, ignore empty lines.
fn riter_lines(buf: &[u8]) -> impl Iterator<Item = &[u8]> + '_ {
    // must ignore empty lines, because the way we detect the end of a line is
    // check either '\r' or '\n', for '\r\n' or '\n\r' we will get extra empty line.
    let mut iter = buf.rsplit(|&c| c == b'\n' || c == b'\r');
    iter.filter(|&line| !line.is_empty())
}

fn parse_tail(buf: &[u8]) -> ParseResult<Tail> {
    let mut iter = riter_lines(buf);
    let line = iter.next().unwrap_or_default();
    complete(tag("%%EOF"))(line)?;
    let line = iter.next().unwrap_or_default();
    map(complete(u32), Tail::new)(line)
}

#[cfg(test)]
mod tests;
