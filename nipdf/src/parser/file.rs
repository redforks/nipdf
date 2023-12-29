use super::{ws_terminated, FileError, ParseError, ParseResult};
use crate::{
    function::{Domain, Domains},
    object::{Dictionary, Entry, Frame, FrameSet, ObjectValueError, RuntimeObjectId, XRefSection},
    parser::{parse_dict, parse_indirect_stream},
};
use log::{error, info};
use memchr::memmem::rfind;
use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{char, satisfy, u32},
    combinator::{complete, map, map_res, opt, recognize},
    error::{context, ErrorKind, ParseError as NomParseError},
    multi::{count, fold_many1, many0},
    number::complete::{be_u16, be_u24, be_u32, be_u8},
    sequence::{preceded, separated_pair, tuple},
    InputIter, InputLength, InputTake, Parser, Slice,
};
use prescript::sname;
use std::{fmt::Display, ops::RangeFrom, str::from_utf8_unchecked};

/// Return `None`` if file not start with `%PDF-`
pub fn parse_header(buf: &[u8]) -> ParseResult<Option<&str>> {
    let one_digit = || satisfy(|c| c.is_ascii_digit());

    fn new_header(buf: &[u8]) -> std::result::Result<Option<&str>, FileError> {
        assert_eq!(3, buf.len());
        if buf[0] != b'1' {
            Err(FileError::UnsupportedVersion(
                String::from_utf8_lossy(buf).to_string(),
            ))
        } else {
            // safe to call from_utf8_unchecked(), because buf is checked for ascii digits
            Ok(Some(unsafe { from_utf8_unchecked(buf) }))
        }
    }

    let (buf, _) = tag("%")(buf)?;
    let (buf, v) = opt(tag("PDF-"))(buf)?;
    if v.is_none() {
        return Ok((buf, None));
    }

    map_res(
        recognize(tuple((one_digit(), char('.'), one_digit()))),
        new_header,
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
    w: Vec<u32>,
}

impl CrossReferenceStreamDict {
    pub fn new(d: &Dictionary) -> Result<Self, ObjectValueError> {
        let size = d
            .get(&sname("Size"))
            .ok_or(ObjectValueError::DictKeyNotFound)?
            .int()? as u32;
        let index = d
            .get(&sname("Index"))
            .map(|o| Domains::<u32>::try_from(o).unwrap());
        let w = d
            .get(&sname("W"))
            .ok_or(ObjectValueError::DictKeyNotFound)?
            .arr()?
            .iter()
            .map(|o| o.int().map(|v| v as u32))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self { size, index, w })
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
fn parse_xref_stream(input: &[u8]) -> ParseResult<(XRefSection, Dictionary)> {
    fn to_parse_error<E: Display>(e: E) -> nom::Err<ParseError<'static>> {
        error!("should be xref table stream: {}", e);
        nom::Err::Error(ParseError::from_error_kind(b"", ErrorKind::Fail))
    }

    let (buf, stream) = parse_indirect_stream(input)?;
    let d = stream.as_dict();
    let d = CrossReferenceStreamDict::new(d).map_err(to_parse_error)?;

    let data = stream
        .decode_without_resolve_length(input, None)
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
            let c: u16 = c.try_into().unwrap();
            let idx: u32 = idx.try_into().unwrap();
            match a {
                0 => r.push((start + idx, Entry::in_file(0, c, false))),
                1 => r.push((start + idx, Entry::in_file(b, c, true))),
                2 => r.push((start + idx, Entry::in_stream(RuntimeObjectId(b), c))),
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
                u32,
                tag(b" "),
                alt((tag(b"n"), tag(b"f"))),
            ))),
            |(offset, _, generation, _, ty)| {
                // safe because clamp into u16 range
                #[allow(clippy::cast_possible_truncation)]
                Entry::in_file(
                    offset,
                    // saturation to u16 for incorrect pdf file, they may use 65536
                    // which out of the range of u16
                    generation.clamp(0, u16::MAX as u32) as u16,
                    ty == b"n",
                )
            },
        ),
    );
    let group = tuple((record_count_parser, many0(record_parser)));
    let parser = fold_many1(group, Vec::new, |mut table, ((start, count), entries)| {
        assert_eq!(count as usize, entries.len());
        for (i, entry) in entries.into_iter().enumerate() {
            table.push((start + u32::try_from(i).unwrap(), entry));
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
        frame.trailer.get(&sname("Prev")).map(|o| o.int().unwrap())
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
