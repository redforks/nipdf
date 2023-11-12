use std::{iter::once, str::from_utf8_unchecked};

use super::Header;
use either::Either;
use winnow::{
    ascii::{escaped, escaped_transform, hex_digit1, line_ending},
    combinator::{alt, delimited, dispatch, fail, fold_repeat, opt, preceded, terminated},
    stream::{AsChar, Stream},
    token::{any, none_of, tag, take_till0, take_till1, take_while},
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

fn is_delimiter(b: u8) -> bool {
    b == b'('
        || b == b')'
        || b == b'<'
        || b == b'>'
        || b == b'['
        || b == b']'
        || b == b'{'
        || b == b'}'
        || b == b'/'
        || b == b'%'
}

/// not white space and delimiter
fn is_regular_char(b: u8) -> bool {
    !is_white_space(b) && !is_delimiter(b)
}

/// Parses one or more white space bytes
fn white_space<'a>(input: &mut &'a [u8]) -> PResult<&'a [u8]> {
    take_while(1.., is_white_space).parse_next(input)
}

/// Matches '\n', '\r', '\r\n'
fn loose_line_ending(input: &mut &[u8]) -> PResult<()> {
    match input.get(0) {
        Some(b'\n') => {
            input.next_token();
            Ok(())
        }
        Some(b'\r') => {
            input.next_token();
            if input.get(0) == Some(&b'\n') {
                input.next_token();
            }
            Ok(())
        }
        _ => fail.parse_next(input),
    }
}

fn int_or_float(input: &mut &[u8]) -> PResult<Either<i32, f32>> {
    let buf =
        take_while(1.., ('0'..='9', 'a'..='z', 'A'..='Z', '.', '-', '+', '#')).parse_next(input)?;
    if let Some(pos) = memchr::memchr(b'#', buf) {
        let (radix, num) = buf.split_at(pos);
        let radix = unsafe { from_utf8_unchecked(radix).parse::<u32>().unwrap() };
        let num = unsafe { i32::from_str_radix(from_utf8_unchecked(&num[1..]), radix).unwrap() };
        return Ok(Either::Left(num));
    }

    if memchr::memchr3(b'.', b'e', b'E', buf).is_some() {
        Ok(Either::Right(unsafe {
            from_utf8_unchecked(buf).parse::<f32>().unwrap()
        }))
    } else {
        Ok(unsafe {
            let s = from_utf8_unchecked(buf);
            s.parse::<i32>()
                .ok()
                .map_or_else(|| Either::Right(s.parse::<f32>().unwrap()), Either::Left)
        })
    }
}

fn string(input: &mut &[u8]) -> PResult<Box<[u8]>> {
    enum StringFragment<'a> {
        Literal(&'a [u8]),
        EscapedChar(u8),
        EscapedNewLine,
        Nested(Box<[u8]>),
    }

    fn literal_fragment<'a>(input: &mut &'a [u8]) -> PResult<StringFragment<'a>> {
        let buf = take_till1((b'(', b')', b'\\')).parse_next(input)?;
        Ok(StringFragment::Literal(buf))
    }

    fn escaped_char<'a>(input: &mut &'a [u8]) -> PResult<StringFragment<'a>> {
        fn parse_oct_byte(input: &mut &[u8]) -> PResult<u8> {
            let buf = take_while(1..=3, |c: u8| c.is_oct_digit()).parse_next(input)?;
            Ok(unsafe { u16::from_str_radix(from_utf8_unchecked(buf), 8).unwrap() as u8 })
        }

        let c = preceded(
            tag(b"\\"),
            alt((
                b'n'.value(b'\n'),
                b'r'.value(b'\r'),
                b't'.value(b'\t'),
                b'b'.value(b'\x08'),
                b'f'.value(b'\x0C'),
                b'('.value(b'('),
                b')'.value(b')'),
                parse_oct_byte,
            )),
        )
        .parse_next(input)?;
        Ok(StringFragment::EscapedChar(c))
    }

    fn escaped_newline<'a>(input: &mut &'a [u8]) -> PResult<StringFragment<'a>> {
        preceded(tag(b"\\"), loose_line_ending).parse_next(input)?;
        Ok(StringFragment::EscapedNewLine)
    }

    fn build_string(input: &mut &[u8]) -> PResult<Box<[u8]>> {
        fold_repeat(0.., fragment, Vec::new, |mut r, frag| {
            match frag {
                StringFragment::Literal(s) => r.extend_from_slice(s),
                StringFragment::EscapedChar(c) => r.push(c),
                StringFragment::EscapedNewLine => (),
                StringFragment::Nested(s) => {
                    r.extend(once(b'(').chain(s.into_iter().copied()).chain(once(b')')))
                }
            }
            r
        })
        .parse_next(input)
        .map(|x| x.into())
    }

    fn nested<'a>(input: &mut &'a [u8]) -> PResult<StringFragment<'a>> {
        let frag = delimited(b'(', opt(build_string), b')').parse_next(input)?;
        Ok(StringFragment::Nested(match frag {
            Some(s) => s,
            None => (*b"").into(),
        }))
    }

    fn fragment<'a>(input: &mut &'a [u8]) -> PResult<StringFragment<'a>> {
        alt((literal_fragment, escaped_char, escaped_newline, nested)).parse_next(input)
    }

    fn literal_string(input: &mut &[u8]) -> PResult<Box<[u8]>> {
        terminated(build_string, b')').parse_next(input)
    }

    /// String encoded in hex wrapped in "<>", e.g. <0123456789ABCDEF>
    /// White space are ignored, if last byte is missing, it is assumed to be 0.
    fn hex_string(input: &mut &[u8]) -> PResult<Box<[u8]>> {
        let bytes = fold_repeat(
            0..,
            alt((hex_digit1, white_space)),
            Vec::new,
            |mut bytes, frag| {
                if !is_white_space(frag[0]) {
                    bytes.extend(frag)
                }
                bytes
            },
        )
        .map(|mut s| {
            if s.len() % 2 != 0 {
                s.push(b'0');
            }

            let mut bytes = Vec::with_capacity(s.len() / 2);
            for i in (0..s.len()).step_by(2) {
                bytes.push(
                    u8::from_str_radix(unsafe { from_utf8_unchecked(&s[i..i + 2]) }, 16).unwrap(),
                );
            }
            bytes.into()
        });

        terminated(bytes, b'>').parse_next(input)
    }

    fn ascii85(input: &mut &[u8]) -> PResult<Box<[u8]>> {
        delimited(
            b'~',
            take_while(0.., |c| c != b'~').map(|v: &[u8]| {
                ascii85::decode(unsafe { from_utf8_unchecked(v) })
                    .unwrap()
                    .into()
            }),
            b"~>",
        )
        .parse_next(input)
    }

    fn hex_or_85(input: &mut &[u8]) -> PResult<Box<[u8]>> {
        alt((hex_string, ascii85)).parse_next(input)
    }

    dispatch!(any;
        b'(' => literal_string,
        b'<' => hex_or_85,
        _ => fail,
    )
    .parse_next(input)
}

fn executable_name<'a>(input: &mut &'a [u8]) -> PResult<&'a [u8]> {
    take_while(1.., is_regular_char).parse_next(input)
}

fn literal_name<'a>(input: &mut &'a [u8]) -> PResult<&'a [u8]> {
    preceded('/', take_while(.., is_regular_char)).parse_next(input)
}

fn immediately_evaluated_name(input: &mut &[u8]) -> PResult<()> {
    b"//".value(()).parse_next(input)
}

#[cfg(test)]
mod tests;
