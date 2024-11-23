use super::eol_3;
use crate::ParserError;
use winnow::{
    Parser,
    combinator::delimited,
    stream::{AsChar, Compare, Stream, StreamIsPartial},
    token::one_of,
};

/// Parser to parse file header, return pdf file version string, such as "1.7".
pub fn header<'a, S>() -> impl Parser<S, &'a [u8], ParserError>
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
        eol_3(),
    )
}
