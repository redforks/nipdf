use super::{eol3, object_now::dict};
use crate::{
    ParserError,
    object::{Entry, FilePos, Frame, FrameSet, ObjectValueError, XRefSection},
};
use hex::FromHexError;
use log::info;
use prescript::sname;
use std::num::ParseIntError;
use winnow::{
    PResult, Parser,
    ascii::dec_uint,
    combinator::{alt, delimited, preceded, repeat, separated_pair, seq, terminated},
    error::{AddContext, ErrMode, ErrorKind, FromExternalError, ParserError as _},
    stream::{AsChar, Compare, ParseSlice, Stream, StreamIsPartial},
    token::{one_of, take},
};

/// Parser to parse file header, return pdf file version string, such as "1.7".
fn header<'a, S>() -> impl Parser<S, &'a [u8], ParserError>
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
        + ParseSlice<u32>
        + 'a,
    E: winnow::error::ParserError<S> + 'a,
{
    preceded(
        terminated(b"xref".as_slice(), eol3()),
        repeat(
            1..,
            terminated(separated_pair(dec_uint, b' ', dec_uint), eol3()).flat_map(
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
/// ignore trailing empty lines.
fn rev_iter_lines(s: &[u8]) -> impl Iterator<Item = &'_ [u8]> {
    s.rsplit(|&b| b == b'\n')
        .map(|line| {
            if line.ends_with(&[b'\r']) {
                &line[..line.len() - 1]
            } else {
                line
            }
        })
        .skip_while(|line| line.is_empty())
}

const EMPTY_BUF: [u8; 0] = [];

fn parse_file_trailers<'a, E>(buf: &mut &'a [u8]) -> PResult<FrameSet, E>
where
    E: winnow::error::ParserError<&'a [u8]>
        + AddContext<&'a [u8]>
        + FromExternalError<&'a [u8], FromHexError>
        + FromExternalError<&'a [u8], ObjectValueError>
        + FromExternalError<&'a [u8], FromHexError>
        + FromExternalError<&'a [u8], ParseIntError>
        + 'a,
{
    // find start of last cross reference section
    let mut lines = rev_iter_lines(buf);
    let mut line = lines
        .next()
        .ok_or_else(move || ErrMode::from_error_kind(&&(EMPTY_BUF[..]), ErrorKind::Eof))?;
    b"%%EOF".as_slice().parse_next(&mut line)?;
    let mut line = lines
        .next()
        .ok_or_else(move || ErrMode::from_error_kind(&&(EMPTY_BUF[..]), ErrorKind::Eof))?;
    let pos: usize = dec_uint(&mut line)?;
    let mut line = lines
        .next()
        .ok_or_else(move || ErrMode::from_error_kind(&&(EMPTY_BUF[..]), ErrorKind::Eof))?;
    b"startxref".as_slice().parse_next(&mut line)?;
    drop(lines);

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
        let mut frame = (
            xref().context("xref"),
            preceded(
                terminated(b"trailer".as_slice(), eol3()),
                terminated(dict, eol3()),
            )
            .context("trailer"),
            terminated(b"startxref".as_slice(), eol3()).context("startxref"),
            terminated(dec_uint::<_, usize, _>, eol3()).context("startxref pos"),
            terminated(b"%%EOF".as_slice(), eol3()).context("EOF tag"),
        )
            .context("frame");
        let f = frame.parse_next(&mut &buf[pos..])?;
        info!("startxref pos: {}", f.3);
        let f = Frame::new(pos.try_into().unwrap(), f.1, f.0);
        next_pos = get_prev(&f);
        r.push(f);
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        file::{report_peek_err, test_file},
        object::Dictionary,
    };
    use snafu::report;
    use winnow::error::{ContextError, TreeError};

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
        let input = b"line1\r\nline2\r\nline3\n";
        let expected: Vec<&[u8]> = vec![b"line3", b"line2", b"line1"];
        let result: Vec<&[u8]> = rev_iter_lines(input).collect();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_file_trailers() {
        let buf = std::fs::read(test_file("sample_files/normal/pdfreference1.0.pdf")).unwrap();
        let frameset = parse_file_trailers::<ContextError<&'static str>>(&mut &buf[..]).unwrap();
        assert_eq!(2, frameset.len());
        let (f1, f2) = (&frameset[0], &frameset[1]);
        assert_eq!(f1.xref_pos, 116);
        assert_eq!(f2.xref_pos, 1513589);
        assert_eq!(f1.trailer.get(&sname("Size")).unwrap().int().unwrap(), 4963);
        assert_eq!(f2.trailer.get(&sname("Size")).unwrap().int().unwrap(), 1046);
    }
}
