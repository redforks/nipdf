use super::{eol3, object_now::dict, wsc_prefixed0};
use crate::{
    PResult, ParserError,
    object::{Entry, FilePos, Frame, FrameSet, XRefSection},
    parser::wsc_prefixed1,
};
use prescript::sname;
use winnow::{
    Parser,
    ascii::dec_uint,
    combinator::{alt, delimited, fail, preceded, repeat, separated_pair, seq, terminated},
    error::{ErrMode, ErrorKind, ParserError as _, StrContext},
    stream::{AsChar, Compare, ParseSlice, Stream, StreamIsPartial},
    token::{any, one_of, take},
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

fn xref<'a, S>() -> impl Parser<S, XRefSection, ParserError> + 'a
where
    S: Stream<Token = u8, Slice = &'a [u8]>
        + StreamIsPartial
        + Compare<u8>
        + Compare<&'a [u8]>
        + ParseSlice<u32>
        + 'a,
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

fn parse_file_trailers(buf: &mut &[u8]) -> PResult<FrameSet> {
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
        let mut frame = (
            xref().context(StrContext::Label("xref")),
            preceded(wsc_prefixed1(b"trailer".as_slice()), dict)
                .context(StrContext::Label("trailer dict")),
            b"%%EOF".as_slice(),
        )
            .context(StrContext::Label("frame"));
        let f = frame.parse_next(&mut &buf[pos..])?;
        let f = Frame::new(pos.try_into().unwrap(), f.1, f.0);
        next_pos = get_prev(&f);
        r.push(f);
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::{report_peek_err, test_file};
    use snafu::report;

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
        report_peek_err(parse_file_trailers(&mut &buf[..]));
        todo!("compare with expected result");
    }
}
