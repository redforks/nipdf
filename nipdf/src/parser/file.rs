use std::str::from_utf8;

use log::info;
use memchr::memmem::rfind;
use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{char, satisfy},
    character::complete::{u16, u32},
    combinator::{map, map_res, recognize},
    error::{context, ErrorKind, ParseError as NomParseError},
    multi::{fold_many1, many0},
    sequence::{preceded, separated_pair, tuple},
};

use crate::{
    object::{Dictionary, Entry, Frame, FrameSet, Name, XRefSection},
    parser::parse_dict,
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

fn new_r_to_tag<'a>(tag: &'static [u8]) -> impl FnMut(&'a [u8]) -> ParseResult<'a, ()> {
    move |buf| {
        let pos = r_find_start_object_tag(buf, tag);
        if let Some(pos) = pos {
            Ok((&buf[pos..], ()))
        } else {
            Err(nom::Err::Failure(ParseError::from_error_kind(
                buf,
                ErrorKind::Fail,
            )))
        }
    }
}

fn parse_trailer(buf: &[u8]) -> ParseResult<Dictionary> {
    preceded(ws_terminated(tag(b"trailer")), ws_terminated(parse_dict))(buf)
}

// Assumes buf start from xref
fn parse_xref_table(buf: &[u8]) -> ParseResult<XRefSection> {
    let record_count_parser = context(
        "record count",
        ws_terminated(separated_pair(u32, tag(b" "), u32)),
    );
    let record_parser = context(
        "record",
        map(
            ws_terminated(tuple((
                u32,
                tag(b" "),
                u16,
                tag(b" "),
                alt((tag(b"n"), tag(b"f"))),
            ))),
            |(offset, _, generation, _, ty)| Entry::new(offset, generation, ty == b"n"),
        ),
    );
    let group = tuple((record_count_parser, many0(record_parser)));
    let parser = fold_many1(group, Vec::new, |mut table, ((start, count), entries)| {
        assert_eq!(count, entries.len() as u32);
        for (i, entry) in entries.into_iter().enumerate() {
            table.push((start + i as u32, entry));
        }
        table
    });

    preceded(context("xref", ws_terminated(tag(b"xref"))), parser)(buf)
}

fn parse_startxref(buf: &[u8]) -> ParseResult<u32> {
    preceded(ws_terminated(tag(b"startxref")), ws_terminated(u32))(buf)
}

// Assumes buf start from xref
fn parse_frame(buf: &[u8]) -> ParseResult<(Dictionary, Vec<(u32, Entry)>)> {
    map(
        tuple((
            context("xref table", parse_xref_table),
            context("trailer", parse_trailer),
        )),
        |(xref_table, trailer)| (trailer, xref_table),
    )(buf)
}

pub fn parse_frame_set(input: &[u8]) -> ParseResult<FrameSet> {
    fn get_prev(frame: &Frame) -> Option<i32> {
        frame
            .trailer
            .get(&Name::borrowed(b"Prev"))
            .map(|o| o.as_int().unwrap())
    }

    let mut frames = Vec::new();
    let (buf, _) = context("move to xref", new_r_to_tag(b"startxref"))(input)?;
    let (_, pos) = context("locate frame pos", parse_startxref)(buf)?;
    info!("frame pos: {}", pos);
    let (_, frame) = parse_frame(&input[pos as usize..])?;
    let frame = Frame::new(pos, frame.0, frame.1);
    let mut prev = get_prev(&frame);
    frames.push(frame);

    while let Some(pos) = prev {
        info!("frame pos: {}", pos);
        let buf = &input[pos as usize..];
        let (_, frame) = parse_frame(buf)?;
        let frame = Frame::new(pos as u32, frame.0, frame.1);
        prev = get_prev(&frame);
        frames.push(frame);
    }

    Ok((&input[..0], frames))
}

#[cfg(test)]
mod tests;
