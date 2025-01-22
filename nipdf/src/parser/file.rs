use super::{dict, eol3, object::zero_prefixed_uint, wsc0, wsc1};
use crate::{
    ObjectValueError,
    function::{Domain, Domains},
    object::{Dictionary, Entry, FilePos, Frame, IndirectObjectDef, RuntimeObjectId},
    parser::{object::indirect_object_def, ws1},
};
use hex::FromHexError;
use log::{info, warn};
use num_traits::{NumCast, Unsigned};
use prescript::sname;
use snafu::{OptionExt, ResultExt};
use std::{
    borrow::Cow,
    fmt::Debug,
    num::{ParseIntError, TryFromIntError},
    str::from_utf8,
};
use winnow::{
    Located, PResult, Parser,
    ascii::{Caseless, dec_uint},
    binary::{be_u8, be_u16, be_u24, be_u32},
    combinator::{alt, delimited, empty, fail, preceded, repeat, separated_pair, seq, terminated},
    error::{AddContext, ContextError, ErrMode, ErrorKind, FromExternalError, ParserError},
    stream::{AsBStr, AsChar, Compare, Location, Stream, StreamIsPartial},
    token::{one_of, take},
};

/// Parser to parse file header, return pdf file version string, such as "1.7".
pub(crate) fn header<'a, S>() -> impl Parser<S, &'a str, crate::ParserError>
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
            .take()
            .try_map(from_utf8),
        eol3(),
    )
}

struct XRefSubSection {
    start_id: u32,
    entries: Vec<FilePos>,
}

impl XRefSubSection {
    fn push_to(self, entries: &mut Vec<(u32, Entry)>) {
        assert!(self.entries.len() < u32::MAX as usize);
        #[allow(clippy::cast_possible_truncation)]
        for (i, entry) in self.entries.into_iter().enumerate() {
            entries.push((self.start_id + i as u32, Entry::InFile(entry)));
        }
    }
}

fn xref<'a, S, E>() -> impl Parser<S, Vec<(u32, Entry)>, E> + 'a
where
    S: Stream<Token = u8, Slice = &'a [u8]>
        + StreamIsPartial
        + Compare<u8>
        + Compare<&'a [u8]>
        + 'a,
    E: ParserError<S> + 'a + AddContext<S> + FromExternalError<S, ParseIntError>,
{
    preceded(
        (wsc0(), b"xref".as_slice(), ws1()),
        repeat(
            1..,
            terminated(separated_pair(dec_uint, b' ', zero_prefixed_uint()), wsc1()).flat_map(
                |(start_id, count): (u32, u32)| {
                    info!("start_id, count: {}/{}", start_id, count);
                    let entry = seq! {
                        FilePos(
                            take(10usize).parse_to(),
                            _: b' ',
                            take(5usize).try_map(|s| {
                                from_utf8(s).unwrap().parse::<u16>().or_else(|e| {
                                    // Many PDFs use 65536 as a special value to indicate that the object is not in use.
                                    if s == b"65536" {
                                        Ok(65535)
                                    } else {
                                        Err(e)
                                    }})
                            }),
                            _: b' ',
                            alt((b'n'.value(true),b'f'.value(false))),
                            _: wsc1() // should be 2 bytes eol, but some invalid pdf file use single \n
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
                .rposition(|&b| b == b'\n' || b == b'\r' || b == b' ')
            {
                // Extract the line after the last EOL
                let line = &self.remaining[eol_pos + 1..];
                // Update the remaining slice to exclude the extracted line and its EOL
                self.remaining = &self.remaining[..eol_pos];
                // Trim any additional trailing EOL characters
                let new_end = self
                    .remaining
                    .iter()
                    .rposition(|&b| b != b'\n' && b != b'\r' || b != b' ')
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
            || Ok(Domains(vec![Domain::new(0, size)])),
            Domains::<u32>::try_from,
        )?;
        let w = d
            .get(&sname("W"))
            .ok_or(ObjectValueError::DictKeyNotFound)?
            .as_arr()?
            .iter()
            .map(|o| o.int().map(|v| v as u32))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self { index, w })
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
fn segment_parser<'a, T, S, E>(n: u32, default_value: T) -> Box<dyn Parser<S, T, E> + 'a>
where
    T: Unsigned + NumCast + Copy + From<u8> + From<u16> + 'static,
    S: Stream<Token = u8, Slice = &'a [u8]> + StreamIsPartial + Compare<u8> + 'a,
    E: ParserError<S> + FromExternalError<S, ObjectValueError> + 'a,
{
    use num_traits::cast;

    match n {
        0 => Box::new(empty.value(default_value)),
        1 => Box::new(be_u8.output_into()),
        2 => Box::new(be_u16.output_into()),
        3 => Box::new(be_u24.try_map(|v| cast(v).whatever_context("Cast from u32"))),
        4 => Box::new(be_u32.try_map(|v| cast(v).whatever_context("Cast from u32"))),
        _ => Box::new(fail),
    }
}

fn parse_xref_stream<'a, S, E>(input: &mut S) -> PResult<(Vec<(u32, Entry)>, Dictionary), E>
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
    E: ParserError<S>
        + 'a
        + ParserError<&'a [u8]>
        + FromExternalError<S, ObjectValueError>
        + FromExternalError<S, FromHexError>
        + FromExternalError<S, ParseIntError>
        + FromExternalError<S, TryFromIntError>
        + FromExternalError<S, ObjectValueError>
        + AddContext<S>,
{
    let start = input.checkpoint();
    let IndirectObjectDef(_, s) = indirect_object_def::<S, E>()
        .context("read xref stream")
        .parse_next(input)?;
    let s = s
        .as_stream()
        .map_err(|e| ErrMode::from_external_error(input, ErrorKind::Fail, e))?;
    let d = CrossReferenceStreamDict::new(s.as_dict())
        .map_err(|e| ErrMode::from_external_error(input, ErrorKind::Fail, e))?;
    input.reset(&start);
    let buf: &'a [u8] = input.finish();
    let data: Cow<'a, [u8]> = s
        .decode_without_resolve_length(buf, None)
        .map_err(|e| ErrMode::from_external_error(input, ErrorKind::Fail, e))?;
    let (a, b, c) = (d.w[0], d.w[1], d.w[2]);
    debug_assert_eq!(
        data.len() % (a + b + c) as usize,
        0,
        "stream data length should multiple of w0 + w1 + w2"
    );

    let count = d.iter_ids().count();
    let mut id_iter = d.iter_ids();
    let r = repeat(
        count,
        (
            segment_parser::<u32, _, ContextError>(a, 1),
            segment_parser::<u32, _, ContextError>(b, 0),
            segment_parser::<u16, _, ContextError>(c, 0),
        ),
    )
    .fold(
        || Ok(Vec::new()),
        |r: Result<_, ObjectValueError>, (a, b, c)| {
            let mut r = r?;
            let next_id = id_iter
                .next()
                .whatever_context::<_, ObjectValueError>("expect more entries in XRefStream")?;
            match a {
                0 => r.push((next_id, Entry::in_file(0, c, false))),
                1 => r.push((next_id, Entry::in_file(b, c, true))),
                2 => r.push((next_id, Entry::in_stream(RuntimeObjectId(b), c))),
                _ => warn!(
                    "unknown xref stream entry type: {}, idx: {}, ignored",
                    a, next_id
                ),
            }
            Ok(r)
        },
    )
    .parse_next(&mut data.as_ref())
    .map_err(|e| {
        warn!("Error when parse xref stream entries: {:?}", e);
        ErrMode::from_error_kind(input, ErrorKind::Fail)
    })?;
    Ok((
        r.map_err(|e| ErrMode::from_external_error(input, ErrorKind::Fail, e))?,
        s.as_dict().clone(),
    ))
}

pub(crate) fn parse_frame_set<'a, E>(buf: &mut &'a [u8]) -> PResult<Vec<Frame>, E>
where
    E: ParserError<&'a [u8]>
        + ParserError<&'a [u8]>
        + ParserError<Located<&'a [u8]>>
        + AddContext<&'a [u8]>
        + AddContext<Located<&'a [u8]>>
        + Debug
        + FromExternalError<Located<&'a [u8]>, ObjectValueError>
        + FromExternalError<Located<&'a [u8]>, FromHexError>
        + FromExternalError<Located<&'a [u8]>, ParseIntError>
        + FromExternalError<Located<&'a [u8]>, TryFromIntError>
        + FromExternalError<Located<&'a [u8]>, ObjectValueError>
        + for<'b> FromExternalError<&'b [u8], ObjectValueError>
        + 'a,
{
    let mut bytes = buf.finish();
    // Trim bytes after %%EOF search from end
    let mut pos = 0;
    for (i, w) in bytes.windows(5).rev().enumerate() {
        if w == b"%%EOF" {
            pos = bytes.len() - i;
            break;
        }
    }
    bytes = &bytes[..pos];
    // find start of last cross reference section
    let mut lines = rev_iter_lines(bytes);
    let mut line = lines
        .next()
        .ok_or_else(|| ErrMode::from_error_kind(buf, ErrorKind::Eof))?;
    b"%%EOF".as_slice().context("EOF").parse_next(&mut line)?;
    let mut line = lines
        .next()
        .ok_or_else(|| ErrMode::from_error_kind(buf, ErrorKind::Eof))?;
    let pos: usize = zero_prefixed_uint().parse_next(&mut line)?;
    let mut line = lines
        .next()
        .ok_or_else(|| ErrMode::from_error_kind(buf, ErrorKind::Eof))?;
    b"startxref".as_slice().parse_next(&mut line)?;

    let mut r = Vec::new();
    let mut next_pos = Some(pos);
    let mut seen_positions = vec![];
    while let Some(pos) = next_pos {
        if seen_positions.contains(&pos) {
            warn!("Detected recursive xref section at position: {}", pos);
            break;
        }
        seen_positions.push(pos);

        info!("trailer frame pos: {}", pos);
        let mut frame = alt((
            parse_xref_stream.context("xref stream"),
            (
                xref().context("xref"),
                seq! {
                    (
                        _: wsc0(),
                        _: b"trailer".as_slice().context("trailer"),
                        _: wsc0(),
                        dict.context("trailer dict"),
                        _: ws1(),
                    )
                }
                .map(|v| v.0),
            )
                .context("xref table"),
        ))
        .context("frame");
        if pos >= bytes.len() {
            return Err(ErrMode::from_error_kind(buf, ErrorKind::Eof));
        }
        let mut bytes = Located::new(&bytes[pos..]);
        let f = frame.parse_next(&mut bytes)?;
        let f = Frame::new(f.1, f.0);
        next_pos = f
            .trailer
            .get(&sname("Prev"))
            .map(|o| {
                o.int()?
                    .try_into()
                    .whatever_context::<_, ObjectValueError>("Prev to usize")
            })
            .transpose()
            .map_err(|e| ErrMode::from_external_error(&bytes, ErrorKind::Fail, e))?;
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
    fn test_xref_section() -> Result<(), crate::ParserError> {
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

        let buf = b"xref
0 0000000003
0000000000 65535 n
0000000015 00000 n
0000000214 00000 n
";
        assert_eq!(
            vec![
                (0, Entry::InFile(FilePos(0, 65535, true))),
                (1, Entry::InFile(FilePos(15, 0, true))),
                (2, Entry::InFile(FilePos(214, 0, true))),
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
        let frameset = parse_frame_set::<ContextError<&'static str>>(&mut &buf[..]).unwrap();
        assert_eq!(2, frameset.len());
        let (f1, f2) = (&frameset[0], &frameset[1]);
        assert_eq!(f1.trailer.get(&sname("Size")).unwrap().int().unwrap(), 4963);
        assert_eq!(f2.trailer.get(&sname("Size")).unwrap().int().unwrap(), 1046);
    }

    #[test]
    fn test_file_trailers_xref_stream() {
        let buf = std::fs::read(test_file("sample_files/bizarre/imm5257b_1.pdf")).unwrap();
        let frameset = parse_frame_set::<ContextError<&'static str>>(&mut &buf[..]).unwrap();
        assert_eq!(2, frameset.len());
    }

    #[test]
    fn whitespace_before_trailer() {
        // 这个文件的 trailer 行之前有个多余的空行
        let buf = std::fs::read(test_file("pdf.js/test/pdfs/ccitt_EndOfBlock_false.pdf")).unwrap();
        let frameset = parse_frame_set::<ContextError<&'static str>>(&mut &buf[..]).unwrap();
        assert_eq!(1, frameset.len());
    }
}
