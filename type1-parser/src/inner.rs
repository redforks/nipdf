use super::Header;
use winnow::{
    ascii::line_ending,
    combinator::{alt, delimited, preceded},
    token::{tag, take_till0, take_till1, take_while},
    PResult, Parser,
};

/// Parses the header of a Type 1 font. The header is the first line of the
/// file, and is of the form:
///
///    %!PS-AdobeFont-1.0: Times-Roman 001.001
///
/// The first token is the version of the Type 1 specification that the font
/// conforms to. The second token is the font name. The third token is the
/// font version.
fn header(input: &mut &[u8]) -> PResult<Header> {
    preceded(tag(b"%!"), alt((b"PS-AdobeFont", b"AdobeFont"))).parse_next(input)?;
    let spec_ver = delimited('-', take_till1(':'), b": ").parse_next(input)?;
    let font_name = take_till1(' ').parse_next(input)?;
    let font_ver =
        delimited(' ', take_while(1.., (('0'..='9'), '.')), line_ending).parse_next(input)?;

    Ok(Header {
        spec_ver: String::from_utf8(spec_ver.to_owned()).unwrap(),
        font_name: String::from_utf8(font_name.to_owned()).unwrap(),
        font_ver: String::from_utf8(font_ver.to_owned()).unwrap(),
    })
}

fn comment(input: &mut &[u8]) -> PResult<()> {
    preceded(
        tag(b"%"),
        take_till0(|c| c == b'\n' || c == b'\r' || c == b'\x0c'),
    )
    .parse_next(input)?;
    Ok(())
}

/// 0x0, 0x9, 0x0A, 0x0C, 0x0D, 0x20
fn is_white_space(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == b'\n' || b == b'\x0C' || b == b'\r' || b == b'\0'
}

#[cfg(test)]
mod tests;
