use std::collections::BTreeMap;

use memchr::memmem::rfind;
use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{char, line_ending, satisfy},
    character::complete::{u16, u32},
    combinator::{complete, map, map_res, recognize, value},
    multi::{fold_many1, many0},
    sequence::{preceded, separated_pair, terminated, tuple},
};

use crate::{
    file::{Header, Tail, Trailer},
    object::{XRefEntry, XRefTableSection},
    parser::parse_dict,
};

use super::{ws, ws_terminated, FileError, ParseError, ParseResult};

fn parse_header(buf: &[u8]) -> ParseResult<'_, Header<'_>> {
    let one_digit = || satisfy(|c| c.is_ascii_digit());

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

/// nom parser consumes buf to the next line of the last object tag.
fn after_tag_r<'a>(buf: &'a [u8], tag: &'static [u8]) -> ParseResult<'a, ()> {
    let pos = r_find_start_object_tag(buf, tag);
    if let Some(pos) = pos {
        let buf = &buf[pos + tag.len()..];
        value((), line_ending)(buf)
    } else {
        Err(nom::Err::Error(ParseError::InvalidFile))
    }
}

fn parse_tail(buf: &[u8]) -> ParseResult<Tail> {
    fn parse(buf: &[u8]) -> ParseResult<Tail> {
        map(complete(terminated(u32, ws(tag(b"%%EOF")))), Tail::new)(buf)
    }
    let (buf, _) = after_tag_r(buf, b"startxref")?;
    parse(buf)
}

fn parse_trailer(buf: &[u8]) -> ParseResult<Trailer> {
    let (buf, _) = after_tag_r(buf, b"trailer")?;
    map(parse_dict, Trailer::new)(buf)
}

fn parse_xref_table_section(buf: &[u8]) -> ParseResult<XRefTableSection> {
    let record_count_parser = ws_terminated(separated_pair(u32, tag(b" "), u32));
    let record_parser = map(
        ws_terminated(tuple((
            u32,
            tag(b" "),
            u16,
            tag(b" "),
            alt((tag(b"n"), tag(b"f"))),
        ))),
        |(offset, _, generation, _, ty)| XRefEntry::new(offset, generation, ty == b"n"),
    );
    let group = tuple((record_count_parser, many0(record_parser)));
    let mut parser = map(
        fold_many1(
            group,
            BTreeMap::new,
            |mut table, ((start, count), entries)| {
                assert_eq!(count, entries.len() as u32);
                for (i, entry) in entries.into_iter().enumerate() {
                    table.insert(start + i as u32, entry);
                }
                table
            },
        ),
        XRefTableSection::new,
    );

    let (buf, _) = after_tag_r(buf, b"xref")?;
    parser(buf)
}

#[cfg(test)]
mod tests;
