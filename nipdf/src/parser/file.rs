use super::{ws_terminated, FileError, ParseError, ParseResult};
use crate::{
    function::{Domain, Domains},
    object::{Dictionary, Entry, Frame, FrameSet, ObjectValueError, XRefSection},
    parser::{parse_dict, parse_indirect_stream},
};
use log::{error, info};
use memchr::memmem::rfind;
use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{char, satisfy, u16, u32},
    combinator::{complete, map, map_res, recognize},
    error::{context, ErrorKind, ParseError as NomParseError},
    multi::{count, fold_many1, many0},
    number::complete::{be_u16, be_u24, be_u32, be_u8},
    sequence::{preceded, separated_pair, tuple},
    InputIter, InputLength, InputTake, Parser, Slice,
};

use prescript_macro::name;
use std::{fmt::Display, num::NonZeroU32, ops::RangeFrom, str::from_utf8};

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

struct CrossReferenceStreamDict {
    size: u32,
    index: Option<Domains<u32>>,
    prev: Option<u32>,
    w: Vec<u32>,
}

impl CrossReferenceStreamDict {
    pub fn new(d: &Dictionary) -> Result<Self, ObjectValueError> {
        let size = d
            .get(&name!("Size"))
            .ok_or(ObjectValueError::DictKeyNotFound)?
            .as_int()? as u32;
        let index = d
            .get(&name!("Index"))
            .map(|o| Domains::<u32>::try_from(o).unwrap());
        let prev = d
            .get(&name!("Prev"))
            .map(|o| o.as_int().map(|v| v as u32))
            .transpose()?;
        let w = d
            .get(&name!("W"))
            .ok_or(ObjectValueError::DictKeyNotFound)?
            .as_arr()?
            .iter()
            .map(|o| o.as_int().map(|v| v as u32))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            size,
            index,
            prev,
            w,
        })
    }
}

/// Return nom parser to parse u32 value by byte length (0, 1, 2, 3, 4),
/// if n is 0, return parser takes 0 bytes and returns 0.
/// if n > 1, n32 stored in big endian n bytes.
fn segment_parser<'a, I: 'a>(n: u32) -> Box<dyn Parser<I, u32, nom::error::VerboseError<I>> + 'a>
where
    I: InputIter<Item = u8> + InputLength + InputTake + Slice<RangeFrom<usize>>,
{
    match n {
        0 => Box::new(|v| Ok((v, 0_u32))),
        1 => Box::new(map(be_u8, |v| v as u32)),
        2 => Box::new(map(be_u16, |v| v as u32)),
        3 => Box::new(be_u24),
        4 => Box::new(be_u32),
        _ => unreachable!(),
    }
}

/// Parse xref from cross-reference streams
fn parse_xref_stream(buf: &[u8]) -> ParseResult<(XRefSection, Dictionary<'_>)> {
    fn to_parse_error<E: Display>(e: E) -> nom::Err<ParseError<'static>> {
        error!("should be xref table stream: {}", e);
        nom::Err::Error(ParseError::from_error_kind(b"", ErrorKind::Fail))
    }

    let (buf, stream) = parse_indirect_stream(buf)?;
    let d = stream.as_dict();
    let d = CrossReferenceStreamDict::new(d).map_err(to_parse_error)?;
    assert!(
        d.prev.is_none(),
        "cross-reference streams with multi frame not supported"
    );

    let data = stream
        .decode_without_resolve_length()
        .map_err(to_parse_error)?;
    assert_eq!(3, d.w.len());
    let (a, b, c) = (d.w[0], d.w[1], d.w[2]);
    debug_assert_eq!(
        data.len() % (a + b + c) as usize,
        0,
        "stream data length should multiple of w0 + w1 + w2"
    );
    let (_, entries) = complete(count(
        move |i| tuple((segment_parser(a), segment_parser(b), segment_parser(c)))(i),
        data.len() / (a + b + c) as usize,
    ))(data.as_ref())
    .map_err(to_parse_error)?;

    let size = d.size;
    let sections = d
        .index
        .unwrap_or_else(|| Domains(vec![Domain::new(0, size)]));

    let mut r = Vec::with_capacity(sections.0.iter().map(|x| x.end as usize).sum());
    let mut entries = entries.into_iter();
    for Domain { start, end: size } in sections.0 {
        for (idx, (a, b, c)) in entries.by_ref().take(size as usize).enumerate() {
            match a {
                0 => r.push((start + idx as u32, Entry::in_file(0, c as u16, false))),
                1 => r.push((start + idx as u32, Entry::in_file(b, c as u16, true))),
                2 => r.push((
                    start + idx as u32,
                    Entry::in_stream(NonZeroU32::new(b).unwrap(), c as u16),
                )),
                _ => info!("unknown xref stream entry type: {a}, idx: {idx}, ignored",),
            }
        }
    }

    Ok((buf, (r, stream.take_dict())))
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
            |(offset, _, generation, _, ty)| Entry::in_file(offset, generation, ty == b"n"),
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
fn parse_frame(buf: &[u8]) -> ParseResult<(Dictionary, XRefSection)> {
    map(
        alt((
            tuple((
                context("xref table", parse_xref_table),
                context("trailer", parse_trailer),
            )),
            parse_xref_stream,
        )),
        |(xref_table, trailer)| (trailer, xref_table),
    )(buf)
}

pub fn parse_frame_set(input: &[u8]) -> ParseResult<FrameSet> {
    fn get_prev(frame: &Frame) -> Option<i32> {
        frame
            .trailer
            .get(&name!("Prev"))
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
