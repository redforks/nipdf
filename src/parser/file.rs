use std::{collections::BTreeMap, str::from_utf8};

use memchr::memmem::rfind;
use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{char, line_ending, satisfy},
    character::complete::{u16, u32},
    combinator::{map, map_res, recognize, value},
    multi::{fold_many1, many0},
    sequence::{preceded, separated_pair, terminated, tuple},
};

use crate::{
    file::File,
    object::{Dictionary, Entry, Frame, FrameSet, Name, XRefSection},
    parser::{parse_dict, ws_prefixed},
};

use super::{ws_terminated, FileError, ParseError, ParseResult};

pub fn parse_header(buf: &[u8]) -> ParseResult<&str> {
    let one_digit = || satisfy(|c| c.is_ascii_digit());

    fn new_header(buf: &[u8]) -> std::result::Result<&str, FileError> {
        assert_eq!(3, buf.len());
        if buf[0] != b'1' {
            Err(FileError::UnsupportedVersion(
                String::from_utf8_lossy(buf).to_string(),
            ))
        } else {
            Ok(from_utf8(buf).unwrap())
        }
    }

    ws_terminated(preceded(
        tag("%PDF-"),
        map_res(
            recognize(tuple((one_digit(), char('.'), one_digit()))),
            new_header,
        ),
    ))(buf)
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

/// nom parser consumes buf to the start of the last object tag.
fn to_tag_r<'a>(buf: &'a [u8], tag: &'static [u8]) -> ParseResult<'a, ()> {
    let pos = r_find_start_object_tag(buf, tag);
    if let Some(pos) = pos {
        Ok((&buf[pos..], ()))
    } else {
        Err(nom::Err::Error(ParseError::InvalidFile))
    }
}

fn parse_trailer(buf: &[u8]) -> ParseResult<Dictionary> {
    preceded(ws_terminated(tag(b"trailer")), ws_terminated(parse_dict))(buf)
}

// Assumes buf start from xref
fn parse_xref_table(buf: &[u8]) -> ParseResult<XRefSection> {
    let record_count_parser = ws_terminated(separated_pair(u32, tag(b" "), u32));
    let record_parser = map(
        ws_terminated(tuple((
            u32,
            tag(b" "),
            u16,
            tag(b" "),
            alt((tag(b"n"), tag(b"f"))),
        ))),
        |(offset, _, generation, _, ty)| Entry::new(offset, generation, ty == b"n"),
    );
    let group = tuple((record_count_parser, many0(record_parser)));
    let parser = fold_many1(
        group,
        BTreeMap::new,
        |mut table, ((start, count), entries)| {
            assert_eq!(count, entries.len() as u32);
            for (i, entry) in entries.into_iter().enumerate() {
                table.insert(start + i as u32, entry);
            }
            table
        },
    );

    preceded(ws_terminated(tag(b"xref")), parser)(buf)
}

fn parse_startxref(buf: &[u8]) -> ParseResult<u32> {
    preceded(ws_terminated(tag(b"startxref")), ws_terminated(u32))(buf)
}

fn parse_eof(buf: &[u8]) -> ParseResult<()> {
    value((), ws_terminated(tag(b"%%EOF")))(buf)
}

// Assumes buf start from xref
fn parse_frame(buf: &[u8]) -> ParseResult<Frame> {
    map(
        tuple((parse_xref_table, parse_trailer, parse_startxref, parse_eof)),
        |(xref_table, trailer, startxref, _)| Frame::new(startxref, trailer, xref_table),
    )(buf)
}

pub fn parse_frame_set(input: &[u8]) -> ParseResult<FrameSet> {
    fn get_prev(frame: &Frame) -> Option<i32> {
        frame
            .trailer
            .get(&Name::new(b"/Prev".as_slice()))
            .map(|o| o.as_int().unwrap())
    }

    let mut frames = Vec::new();
    let (buf, _) = to_tag_r(input, b"startxref")?;
    let (_, pos) = parse_startxref(buf)?;
    let (_, frame) = parse_frame(&input[pos as usize..])?;
    let mut prev = get_prev(&frame);
    frames.push(frame);

    while let Some(pos) = prev {
        let buf = &input[pos as usize..];
        let (_, frame) = parse_frame(buf)?;
        prev = get_prev(&frame);
        frames.push(frame);
    }

    Ok((&input[..0], frames))
}

pub fn parse_file(_input: &[u8]) -> ParseResult<File> {
    todo!()
    // let (buf, header) = parse_header(input)?;
    // let (buf, frame_set) = parse_frame_set(buf)?;
    // Ok((buf, File::new(input, header, frame_set)))
}

#[cfg(test)]
mod tests;
