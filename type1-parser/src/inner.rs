use super::Header;
use winnow::{
    ascii::line_ending,
    combinator::{alt, delimited, preceded},
    token::{tag, take_till1, take_while},
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
fn parse_header(input: &mut &[u8]) -> PResult<Header> {
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

#[cfg(test)]
mod tests;
