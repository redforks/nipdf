use memchr::memmem::rfind;
use nom::{
    bytes::complete::tag,
    character::complete::u32,
    character::complete::{char, line_ending, satisfy},
    combinator::{complete, map, map_res, recognize},
    sequence::{delimited, preceded, terminated, tuple},
};

use crate::{
    file::{Header, Tail, Trailer},
    parser::ws_terminated,
};

use super::{ws, ws_prefixed, FileError, ParseError, ParseResult};

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

/// Return start position of object tag from the end of the buffer.
/// Object tag occupies a whole line.
fn r_find_start_object_tag(mut buf: &[u8], tag: &[u8]) -> Option<usize> {
    fn is_new_line(buf: &[u8], pos: usize) -> bool {
        buf.get(pos).map_or(true, |ch| *ch == b'\n' || *ch == b'\r')
    }

    loop {
        match rfind(buf, tag) {
            None => return None,
            Some(pos) => {
                if is_new_line(buf, pos + tag.len()) && (pos == 0 || is_new_line(buf, pos - 1)) {
                    return Some(pos);
                } else {
                    buf = &buf[..pos];
                }
            }
        }
    }
}

/// nom parser consumes buf until the end of the last object tag.
fn after_tag_r<'a>(buf: &'a [u8], tag: &'static [u8]) -> ParseResult<'a, ()> {
    let pos = r_find_start_object_tag(buf, tag);
    if let Some(pos) = pos {
        Ok((&buf[pos + tag.len()..], ()))
    } else {
        Err(nom::Err::Error(ParseError::InvalidFile))
    }
}

fn parse_tail(buf: &[u8]) -> ParseResult<Tail> {
    fn parse(buf: &[u8]) -> ParseResult<Tail> {
        map(
            complete(terminated(ws_prefixed(u32), ws(tag(b"%%EOF")))),
            Tail::new,
        )(buf)
    }
    let (buf, _) = after_tag_r(buf, b"startxref")?;
    parse(buf)
}

#[cfg(test)]
mod tests;
