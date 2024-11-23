use super::eol3;
use crate::{
    ParserError,
    object::{Entry, FilePos, XRefSection},
};
use winnow::{
    Parser,
    ascii::dec_uint,
    combinator::{alt, delimited, fail, preceded, repeat, separated_pair, seq, terminated},
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

fn xref_section<'a, S>() -> impl Parser<S, XRefSection, ParserError> + 'a
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

#[cfg(test)]
mod tests {
    use super::*;
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
            xref_section().parse(&buf[..])?
        );
        Ok(())
    }
}
