use super::{dict, eol3, wsc0, wsc1};
use crate::{
    ParserError,
    function::{Domain, Domains},
    object::{
        Dictionary, Entry, FilePos, Frame, FrameSet, IndirectObjectDef, ObjectValueError,
        RuntimeObjectId, XRefSection,
    },
    parser::object_now::indirect_object_def,
};
use hex::FromHexError;
use log::{info, warn};
use prescript::sname;
use std::{fmt::Debug, num::ParseIntError};
use winnow::{
    Located, PResult, Parser,
    ascii::{Caseless, dec_uint},
    binary::{be_u8, be_u16, be_u24, be_u32},
    combinator::{alt, delimited, empty, preceded, repeat, separated_pair, seq, terminated},
    error::{AddContext, ErrMode, ErrorKind, FromExternalError, ParserError as _},
    stream::{AsBStr, AsChar, Compare, Location, Stream, StreamIsPartial},
    token::{one_of, take},
};

/// Parser to parse file header, return pdf file version string, such as "1.7".
pub(crate) fn header<'a, S>() -> impl Parser<S, &'a [u8], ParserError>
where
    S: Stream<Token = u8, Slice = &'a [u8]>
        + StreamIsPartial
        + Compare<u8>
        + Compare<&'a [u8]>
        + 'a,
{
    delimited(
        b"%PDF-".as_slice(),
        (
            one_of(AsChar::is_dec_digit),
            b'.',
            one_of(AsChar::is_dec_digit),
        )
            .take(),
        eol3(),
    )
}

struct XRefSubSection {
    start_id: u32,
    entries: Vec<FilePos>,
}

impl XRefSubSection {
    fn push_to(self, entries: &mut Vec<(u32, Entry)>) {
        for (i, entry) in self.entries.into_iter().enumerate() {
            entries.push((self.start_id + i as u32, Entry::InFile(entry)));
        }
    }
}

fn xref<'a, S, E>() -> impl Parser<S, XRefSection, E> + 'a
where
    S: Stream<Token = u8, Slice = &'a [u8]>
        + StreamIsPartial
        + Compare<u8>
        + Compare<&'a [u8]>
        + 'a,
    E: winnow::error::ParserError<S> + 'a,
{
    preceded(
        (wsc0(), b"xref".as_slice(), eol3()),
        repeat(
            1..,
            terminated(separated_pair(dec_uint, b' ', dec_uint), wsc1()).flat_map(
                |(start_id, count): (u32, u32)| {
                    let entry = seq! {
                        FilePos(
                            take(10usize).parse_to(),
                            _: b' ',
                            take(5usize).parse_to(),
                            _: b' ',
                            alt((b'n'.value(true),b'f'.value(false))),
                            _: take(2usize) // 2 bytes eol
                        )
                    };
                    repeat::<_, _, Vec<_>, _, _>(count as usize, entry)
                        .map(move |entries| XRefSubSection { start_id, entries })
                },
            ),
        )
        .fold(Vec::new, |mut r, section| {
            section.push_to(&mut r);
            r
        }),
    )
}

/// Return an iterator that yields lines in reverse order. Return from first non-empty line,
/// ignore trailing empty lines. EOL can be "\n", "\r\n", and "\r".
fn rev_iter_lines(s: &[u8]) -> impl Iterator<Item = &[u8]> {
    LinesRev::new(s)
}

struct LinesRev<'a> {
    remaining: &'a [u8],
}

impl<'a> LinesRev<'a> {
    fn new(s: &'a [u8]) -> Self {
        // Skip any trailing line endings
        let mut end = s.len();
        while end > 0 {
            let b = s[end - 1];
            if b == b'\n' || b == b'\r' {
                end -= 1;
            } else {
                break;
            }
        }
        LinesRev {
            remaining: &s[..end],
        }
    }
}

impl<'a> Iterator for LinesRev<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        while !self.remaining.is_empty() {
            // Find the position of the last EOL character
            if let Some(eol_pos) = self
                .remaining
                .iter()
                .rposition(|&b| b == b'\n' || b == b'\r')
            {
                // Extract the line after the last EOL
                let line = &self.remaining[eol_pos + 1..];
                // Update the remaining slice to exclude the extracted line and its EOL
                self.remaining = &self.remaining[..eol_pos];
                // Trim any additional trailing EOL characters
                let new_end = self
                    .remaining
                    .iter()
                    .rposition(|&b| b != b'\n' && b != b'\r')
                    .map_or(0, |pos| pos + 1);
                self.remaining = &self.remaining[..new_end];
                // Return the line if it's not empty
                if !line.is_empty() {
                    return Some(line);
                }
            } else {
                // No more EOL characters; return the remaining slice if not empty
                let line = self.remaining;
                self.remaining = &[];
                if !line.is_empty() {
                    return Some(line);
                }
            }
        }
        None
    }
}

struct CrossReferenceStreamDict {
    size: u32,
    index: Domains<u32>,
    w: Vec<u32>,
}

impl CrossReferenceStreamDict {
    pub fn new(d: &Dictionary) -> Result<Self, ObjectValueError> {
        let size = d
            .get(&sname("Size"))
            .ok_or(ObjectValueError::DictKeyNotFound)?
            .int()? as u32;
        let index = d.get(&sname("Index")).map_or_else(
            || Domains(vec![Domain::new(0, size)]),
            |o| Domains::<u32>::try_from(o).unwrap(),
        );
        let w = d
            .get(&sname("W"))
            .ok_or(ObjectValueError::DictKeyNotFound)?
            .arr()?
            .iter()
            .map(|o| o.int().map(|v| v as u32))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self { size, index, w })
    }

    pub fn iter_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.index
            .iter()
            .flat_map(move |d| d.start..(d.start + d.end))
    }
}

/// Return nom parser to parse u32 value by byte length (0, 1, 2, 3, 4),
/// if n is 0, return parser takes 0 bytes and returns `default_value`
/// if n > 1, n32 stored in big endian n bytes.
fn segment_parser<'a, S, E>(n: u32, default_value: u32) -> Box<dyn Parser<S, u32, E> + 'a>
where
    S: Stream<Token = u8, Slice = &'a [u8]> + StreamIsPartial + Compare<u8> + 'a,
    E: winnow::error::ParserError<S> + 'a,
{
    match n {
        0 => Box::new(empty.value(default_value)),
        1 => Box::new(be_u8.output_into()),
        2 => Box::new(be_u16.output_into()),
        3 => Box::new(be_u24),
        4 => Box::new(be_u32),
        _ => unreachable!(),
    }
}

fn parse_xref_stream<'a, S, E>(input: &mut S) -> PResult<(XRefSection, Dictionary), E>
where
    S: Stream<Token = u8, Slice = &'a [u8]>
        + StreamIsPartial
        + AsBStr
        + Compare<u8>
        + Compare<char>
        + Compare<&'a [u8]>
        + Compare<Caseless<&'static str>>
        + Location
        + 'a,
    <S as Stream>::IterOffsets: Clone,
    E: winnow::error::ParserError<S> + 'a,
    E: winnow::error::ParserError<&'a [u8]> + 'a,
    E: FromExternalError<S, ObjectValueError>,
    E: FromExternalError<S, FromHexError> + 'a,
    E: FromExternalError<S, ParseIntError> + 'a,
    E: AddContext<S>,
{
    let start = input.checkpoint();
    let IndirectObjectDef(_, s) = indirect_object_def::<S, E>().parse_next(input)?;
    let s = s.stream().unwrap().clone();
    let d = CrossReferenceStreamDict::new(s.as_dict())
        .map_err(|e| ErrMode::from_external_error(input, ErrorKind::Fail, e))?;
    input.reset(&start);
    let buf = input.finish();
    let data = s
        .decode_without_resolve_length(buf, None)
        .map_err(|e| ErrMode::from_external_error(input, ErrorKind::Fail, e))?;
    let (a, b, c) = (d.w[0], d.w[1], d.w[2]);
    (a, b, c);
    debug_assert_eq!(
        data.len() % (a + b + c) as usize,
        0,
        "stream data length should multiple of w0 + w1 + w2"
    );

    let mut buf = data.as_ref();
    let count = d.iter_ids().count();
    let mut id_iter = d.iter_ids();
    let r = repeat(
        count,
        (
            segment_parser::<_, ()>(a, 1),
            segment_parser(b, 0),
            segment_parser(c, 0),
        ),
    )
    .fold(Vec::new, |mut r, (a, b, c)| {
        let c: u16 = c.try_into().unwrap();
        match a {
            0 => r.push((id_iter.next().unwrap(), Entry::in_file(0, c, false))),
            1 => r.push((id_iter.next().unwrap(), Entry::in_file(b, c, true))),
            2 => r.push((
                id_iter.next().unwrap(),
                Entry::in_stream(RuntimeObjectId(b), c),
            )),
            _ => warn!(
                "unknown xref stream entry type: {}, idx: {}, ignored",
                a,
                id_iter.next().unwrap()
            ),
        }
        r
    })
    .parse_next(&mut buf)
    .unwrap();
    Ok((r, s.take_dict()))
}

pub(crate) fn parse_frame_set<'a, S, E>(buf: &mut S) -> PResult<FrameSet, E>
where
    S: Stream<Token = u8, Slice = &'a [u8]>
        + StreamIsPartial
        + AsBStr
        + Compare<u8>
        + Compare<char>
        + Compare<&'a [u8]>
        + Compare<Caseless<&'static str>>
        + Clone
        + 'a,
    <S as Stream>::IterOffsets: Clone,
    E: winnow::error::ParserError<S> + winnow::error::ParserError<&'a [u8]> + AddContext<S> + 'a,
    E: winnow::error::ParserError<Located<&'a [u8]>>
        + AddContext<Located<&'a [u8]>>
        + Debug
        + FromExternalError<Located<&'a [u8]>, ObjectValueError>
        + FromExternalError<Located<&'a [u8]>, FromHexError>
        + FromExternalError<Located<&'a [u8]>, ParseIntError>
        + 'a,
{
    let bytes = buf.peek_finish().1;
    // find start of last cross reference section
    let mut lines = rev_iter_lines(bytes);
    let mut line = lines
        .next()
        .ok_or_else(|| ErrMode::from_error_kind(buf, ErrorKind::Eof))?;
    b"%%EOF".as_slice().parse_next(&mut line)?;
    let mut line = lines
        .next()
        .ok_or_else(|| ErrMode::from_error_kind(buf, ErrorKind::Eof))?;
    let pos: usize = dec_uint(&mut line)?;
    let mut line = lines
        .next()
        .ok_or_else(|| ErrMode::from_error_kind(buf, ErrorKind::Eof))?;
    b"startxref".as_slice().parse_next(&mut line)?;

    fn get_prev(frame: &Frame) -> Option<usize> {
        frame
            .trailer
            .get(&sname("Prev"))
            .map(|o| o.int().unwrap().try_into().unwrap())
    }

    let mut r = Vec::new();
    let mut next_pos = Some(pos);
    while let Some(pos) = next_pos {
        info!("trailer frame pos: {}", pos);
        let mut frame = (alt((
            (
                xref().context("xref"),
                preceded(
                    terminated(b"trailer".as_slice(), eol3()),
                    terminated(dict, eol3()),
                )
                .context("trailer"),
            ),
            parse_xref_stream.context("xref stream"),
        )),)
            .context("frame");
        let mut bytes = Located::new(&bytes[pos..]);
        let f = frame.parse_next(&mut bytes)?;
        let f = Frame::new(pos.try_into().unwrap(), f.0.1, f.0.0);
        next_pos = get_prev(&f);
        r.push(f);
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::test_file;
    use snafu::report;
    use winnow::error::ContextError;

    #[report]
    #[test]
    fn test_xref_section() -> Result<(), ParserError> {
        // extra \r add to the end of entry lines because eol should 2 bytes
        let buf = b"xref
0 1
0000000000 65535 f\r
23 2
0000025518 00002 n\r
0000025635 00000 n\r
";
        assert_eq!(
            vec![
                (0, Entry::InFile(FilePos(0, 65535, false))),
                (23, Entry::InFile(FilePos(25518, 2, true))),
                (24, Entry::InFile(FilePos(25635, 0, true))),
            ],
            xref().parse(&buf[..])?
        );
        Ok(())
    }

    #[test]
    fn test_rev_iter_lines() {
        let input = b"line0\rline1\r\nline2\r\nline3\n";
        let expected: Vec<&[u8]> = vec![b"line3", b"line2", b"line1", b"line0"];
        let result: Vec<&[u8]> = rev_iter_lines(input).collect();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_file_trailers() {
        let buf = std::fs::read(test_file("sample_files/normal/pdfreference1.0.pdf")).unwrap();
        let frameset = parse_frame_set::<_, ContextError<&'static str>>(&mut &buf[..]).unwrap();
        assert_eq!(2, frameset.len());
        let (f1, f2) = (&frameset[0], &frameset[1]);
        assert_eq!(f1.xref_pos, 116);
        assert_eq!(f2.xref_pos, 1513589);
        assert_eq!(f1.trailer.get(&sname("Size")).unwrap().int().unwrap(), 4963);
        assert_eq!(f2.trailer.get(&sname("Size")).unwrap().int().unwrap(), 1046);
    }

    #[test]
    fn test_file_trailers_xref_stream() {
        let buf = std::fs::read(test_file("sample_files/bizarre/imm5257b_1.pdf")).unwrap();
        let frameset = parse_frame_set::<_, ContextError<&'static str>>(&mut &buf[..]).unwrap();
        assert_eq!(2, frameset.len());
    }
}
