use super::{eol2, eol3, ws_prefixed0, ws_prefixed1, wsc_prefixed0, wsc0};
use crate::{
    ObjectValueError,
    object::{
        BufPos, Dictionary, HexString, IndirectObjectDef, InnerString, LiteralString, Object,
        ObjectId, Reference, Stream as PdfStream,
    },
    parser::is_whitespace,
};
use ahash::HashMap;
use either::Either;
use hex::FromHexError;
use prescript::Name;
use std::{
    borrow::Cow,
    num::{ParseIntError, TryFromIntError},
};
use winnow::{
    ModalResult, Parser,
    ascii::{Caseless, dec_uint, float},
    combinator::{alt, delimited, opt, preceded, repeat, repeat_till, terminated},
    error::{AddContext, ErrMode, FromExternalError, ParserError},
    stream::{AsBStr, AsChar, Compare, ContainsToken, Location, Stream, StreamIsPartial},
    token::{any, rest, take, take_till, take_while},
};

fn name<'a, S, E>() -> impl Parser<S, Name, E> + 'a
where
    S: Stream<Token = u8, Slice = &'a [u8]> + StreamIsPartial + Compare<u8> + 'a,
    E: ParserError<S> + 'a + FromExternalError<S, ObjectValueError>,
{
    preceded(b'/', take_till(0.., b" \t\r\n\x0C[<(/>]")).try_map(normalize_name)
}

/// Return `Err(ObjectValueError::InvalidNameFormat)` if the name is not a valid PDF name encoding,
/// not two hex char after `#`.
fn normalize_name(buf: &[u8]) -> Result<Name, ObjectValueError> {
    fn next_hex_char(iter: &mut impl Iterator<Item = u8>) -> Option<u8> {
        let hex_str: String = iter.take(2).map(|c| c as char).collect();
        u8::from_str_radix(&hex_str, 16).ok()
    }

    if !buf.contains(&b'#') {
        return Ok(prescript::name(&String::from_utf8_lossy(buf)));
    }

    let mut result = Vec::with_capacity(buf.len());
    let mut iter = buf.iter().copied();
    while let Some(next) = iter.next() {
        if next == b'#' {
            if let Some(c) = next_hex_char(&mut iter) {
                result.push(c);
            } else {
                return Err(ObjectValueError::InvalidNameFormat);
            }
        } else {
            result.push(next);
        }
    }
    Ok(prescript::name(&String::from_utf8_lossy(&result)))
}

fn number<'a, S, E>() -> impl Parser<S, Object, ErrMode<E>> + 'a
where
    S: Stream<Token = u8, Slice = &'a [u8]>
        + StreamIsPartial
        + Compare<u8>
        + Compare<char>
        + AsBStr
        + Compare<Caseless<&'static str>>
        + 'a,
    <S as Stream>::IterOffsets: Clone,
    E: ParserError<S> + 'a + ParserError<&'a [u8]>,
{
    fn fallback<E>(buf: &mut &[u8]) -> ModalResult<Object, E> {
        buf.finish();
        Ok(Object::Integer(0))
    }
    take_while::<_, S, _>(1.., (b'0'..=b'9', b'+', b'-', b'.'))
        .take()
        .and_then(alt((
            rest.parse_to().map(Object::Integer),
            rest.parse_to().map(Object::Number),
            float.map(Object::Number),
            fallback,
        )))
}

#[derive(Clone)]
enum LiteralStringFragment<'a> {
    Literal(&'a [u8]),
    Escaped(u8),
    EscapedLine,
    Nested(LiteralString),
}

fn parse_quoted_string<'a, S, E>(input: &mut S) -> ModalResult<LiteralString, E>
where
    S: Stream<Token = u8, Slice = &'a [u8]> + StreamIsPartial + Compare<u8> + 'a,
    E: ParserError<S> + 'a + FromExternalError<S, ParseIntError>,
{
    let literal = take_till(1.., b"\\()").map(LiteralStringFragment::Literal);
    let paired = parse_quoted_string.map(LiteralStringFragment::Nested);
    let oct_char = take_while(1..4, AsChar::is_oct_digit)
        .try_map(|s: &[u8]| u8::from_str_radix(&String::from_utf8_lossy(s), 8))
        .map(LiteralStringFragment::Escaped);
    let escaped_line = eol3().value(LiteralStringFragment::EscapedLine);
    let escaped = preceded(
        b'\\',
        alt((
            b'n'.value(LiteralStringFragment::Escaped(b'\n')),
            b'r'.value(LiteralStringFragment::Escaped(b'\r')),
            b't'.value(LiteralStringFragment::Escaped(b'\t')),
            b'b'.value(LiteralStringFragment::Escaped(b'\x08')),
            b'f'.value(LiteralStringFragment::Escaped(b'\x0C')),
            oct_char,
            escaped_line,
            any.map(LiteralStringFragment::Escaped),
        )),
    );
    // .map(LiteralStringFragment::Literal);
    delimited(
        b'(',
        repeat(0.., alt((literal, paired, escaped))).fold(InnerString::new, |mut r, f| {
            match f {
                LiteralStringFragment::Literal(s) => r.extend_from_slice(s),
                LiteralStringFragment::Escaped(c) => r.push(c),
                LiteralStringFragment::Nested(mut s) => {
                    r.push(b'(');
                    r.append(&mut s.0);
                    r.push(b')');
                }
                LiteralStringFragment::EscapedLine => {}
            }
            r
        }),
        b')',
    )
    .map(LiteralString)
    .parse_next(input)
}

fn decode_hex(buf: &[u8]) -> Result<HexString, FromHexError> {
    /// Remove whitespace from hex string.
    fn preprocess(buf: &[u8]) -> Cow<'_, [u8]> {
        let is_ws = is_whitespace();
        if (buf.len() % 2 == 0) && !buf.iter().any(|&c| is_ws.contains_token(c)) {
            return buf.into();
        }

        let mut result: Vec<u8> = buf
            .iter()
            .copied()
            .filter(|&c| !is_ws.contains_token(c))
            .collect();
        if result.len() % 2 != 0 {
            result.push(b'0');
        }
        result.into()
    }

    let buf = preprocess(buf);
    hex::decode(&buf).map(|v| HexString((&v[..]).into()))
}

pub(crate) fn hex_string<'a, S, E>() -> impl Parser<S, Object, E> + 'a
where
    S: Stream<Token = u8, Slice = &'a [u8]> + StreamIsPartial + Compare<u8> + 'a,
    E: ParserError<S> + FromExternalError<S, FromHexError> + 'a,
{
    let parser = take_while(
        ..,
        (AsChar::is_hex_digit, [b' ', b'\t', b'\r', b'\n', b'\x0C']),
    );
    delimited(b'<', parser.try_map(decode_hex), b'>').map(Object::HexString)
}

fn array<'a, S, E>(input: &mut S) -> ModalResult<Object, E>
where
    S: Stream<Token = u8, Slice = &'a [u8]>
        + StreamIsPartial
        + AsBStr
        + Compare<u8>
        + Compare<char>
        + Compare<&'a [u8]>
        + Compare<Caseless<&'static str>>
        + 'a,
    <S as Stream>::IterOffsets: Clone,
    E: ParserError<S>
        + 'a
        + ParserError<&'a [u8]>
        + AddContext<S, &'static str>
        + FromExternalError<S, ObjectValueError>
        + FromExternalError<S, FromHexError>
        + FromExternalError<S, ParseIntError>,
{
    let item = repeat::<_, _, Vec<_>, _, _>(0.., wsc_prefixed0(object()));
    delimited(b'[', item, (wsc0(), b']'))
        .output_into()
        .parse_next(input)
}

/// Parse Dictionary body, i.e, Dictionary without '<<' and '>>' quote.
pub(crate) fn dict_body<'a, S, E>() -> impl Parser<S, Dictionary, ErrMode<E>> + 'a
where
    S: Stream<Token = u8, Slice = &'a [u8]>
        + StreamIsPartial
        + AsBStr
        + Compare<u8>
        + Compare<char>
        + Compare<&'a [u8]>
        + Compare<Caseless<&'static str>>
        + 'a,
    <S as Stream>::IterOffsets: Clone,
    E: ParserError<S>
        + 'a
        + ParserError<&'a [u8]>
        + AddContext<S, &'static str>
        + FromExternalError<S, ObjectValueError>
        + FromExternalError<S, FromHexError>
        + FromExternalError<S, ParseIntError>,
{
    let key = wsc_prefixed0(name());
    let value = wsc_prefixed0(object());
    repeat::<_, _, HashMap<_, _>, _, _>(0.., (key, value)).map(Dictionary::from)
}

pub(crate) fn dict<'a, S, E>(input: &mut S) -> ModalResult<Dictionary, E>
where
    S: Stream<Token = u8, Slice = &'a [u8]>
        + StreamIsPartial
        + AsBStr
        + Compare<u8>
        + Compare<char>
        + Compare<&'a [u8]>
        + Compare<Caseless<&'static str>>
        + 'a,
    <S as Stream>::IterOffsets: Clone,
    E: ParserError<S>
        + 'a
        + ParserError<&'a [u8]>
        + AddContext<S, &'static str>
        + FromExternalError<S, ObjectValueError>
        + FromExternalError<S, FromHexError>
        + FromExternalError<S, ParseIntError>,
{
    delimited(
        b"<<".as_slice(),
        dict_body().context("dict body"),
        (wsc0(), b">>".as_slice()),
    )
    .parse_next(input)
}

pub(crate) fn zero_prefixed_uint<'a, T, S, E>() -> impl Parser<S, T, E> + 'a
where
    S: Stream<Token = u8, Slice = &'a [u8]> + StreamIsPartial + Compare<u8> + 'a,
    E: ParserError<S> + 'a,
    T: std::str::FromStr + 'static,
{
    take_while(1.., '0'..='9').parse_to::<T>()
}

pub(crate) fn object_id<'a, S, E>() -> impl Parser<S, ObjectId, E> + 'a
where
    S: Stream<Token = u8, Slice = &'a [u8]> + StreamIsPartial + Compare<u8> + 'a,
    E: ParserError<S> + 'a,
{
    (zero_prefixed_uint(), ws_prefixed1(dec_uint))
        .map(|(id, r#gen): (u32, u16)| ObjectId::new(id, r#gen))
}

fn reference<'a, S, E>() -> impl Parser<S, Object, E> + 'a
where
    S: Stream<Token = u8, Slice = &'a [u8]> + StreamIsPartial + Compare<u8> + 'a,
    E: ParserError<S> + 'a,
{
    terminated(object_id(), ws_prefixed1(b'R'))
        .map(|id| Reference::new(id.id(), id.generation()).into())
}

/// Return parser to parse [Object].
pub(crate) fn object<'a, S, E>() -> impl Parser<S, Object, ErrMode<E>> + 'a
where
    S: Stream<Token = u8, Slice = &'a [u8]>
        + StreamIsPartial
        + AsBStr
        + Compare<u8>
        + Compare<char>
        + Compare<&'a [u8]>
        + Compare<Caseless<&'static str>>
        + 'a,
    <S as Stream>::IterOffsets: Clone,
    E: ParserError<S>
        + 'a
        + ParserError<&'a [u8]>
        + AddContext<S, &'static str>
        + FromExternalError<S, ObjectValueError>
        + FromExternalError<S, FromHexError>
        + FromExternalError<S, ParseIntError>,
{
    let null = b"null".as_slice().value(Object::Null);
    let bool = alt((
        b"true".as_slice().value(Object::Bool(true)),
        b"false".as_slice().value(Object::Bool(false)),
    ));
    let name = name().map(Object::Name);
    let quoted_string = parse_quoted_string.map(Object::LiteralString);

    alt((
        null,
        bool,
        reference(),
        number(),
        name,
        quoted_string,
        hex_string(),
        array,
        dict.output_into(),
    ))
}

/// Return parser to parse [Object] without reference.
///
/// It is used to parse objects of page content stream. In page content stream, object reference
/// is not allowed. It prevents content like `1 1 0 RG` to be parsed as (int, reference, and 'G').
///
/// References inside dictionary is okay.
pub(crate) fn object_inside_page_stream<'a, S, E>() -> impl Parser<S, Object, ErrMode<E>> + 'a
where
    S: Stream<Token = u8, Slice = &'a [u8]>
        + StreamIsPartial
        + AsBStr
        + Compare<u8>
        + Compare<char>
        + Compare<&'a [u8]>
        + Compare<Caseless<&'static str>>
        + 'a,
    <S as Stream>::IterOffsets: Clone,
    E: ParserError<S>
        + 'a
        + ParserError<&'a [u8]>
        + AddContext<S, &'static str>
        + FromExternalError<S, ObjectValueError>
        + FromExternalError<S, FromHexError>
        + FromExternalError<S, ParseIntError>,
{
    let bool = alt((
        b"true".as_slice().value(Object::Bool(true)),
        b"false".as_slice().value(Object::Bool(false)),
    ));
    let name = name().map(Object::Name);
    let quoted_string = parse_quoted_string.map(Object::LiteralString);

    alt((
        bool,
        number(),
        name,
        quoted_string,
        hex_string(),
        array,
        dict.output_into(),
    ))
}

/// Return parser to parse indirect object definition.
///
/// If stream dict length is reference, parser will end at after the `stream<eol>`, because
/// stream length not known at this point.
pub(crate) fn indirect_object_def<'a, S, E>() -> impl Parser<S, IndirectObjectDef, ErrMode<E>> + 'a
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
    E: AddContext<S>
        + 'a
        + ParserError<S>
        + ParserError<&'a [u8]>
        + FromExternalError<S, ObjectValueError>
        + FromExternalError<S, FromHexError>
        + FromExternalError<S, TryFromIntError>
        + FromExternalError<S, ParseIntError>,
{
    (
        wsc_prefixed0(object_id()).context("object id"),
        preceded(
            ws_prefixed0(b"obj".as_slice()).context("obj tag"),
            wsc_prefixed0(indirect_object_content).context("indirect object data"),
        ),
    )
        .map(|(id, dict_or_bufpos)| match dict_or_bufpos {
            Either::Left(o) => IndirectObjectDef(id, o),
            Either::Right((dict, bufpos)) => {
                IndirectObjectDef(id, PdfStream(dict, bufpos, id).into())
            }
        })
}

fn indirect_object_content<'a, S, E>(
    buf: &mut S,
) -> ModalResult<Either<Object, (Dictionary, BufPos)>, E>
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
        + AddContext<S, &'static str>
        + FromExternalError<S, ObjectValueError>
        + FromExternalError<S, FromHexError>
        + FromExternalError<S, TryFromIntError>
        + FromExternalError<S, ParseIntError>,
{
    let o = object().parse_next(buf)?;
    let Object::Dictionary(dict) = o else {
        (wsc0(), b"endobj".as_slice(), wsc0()).parse_next(buf)?;
        return Ok(Either::Left(o));
    };
    let len: Option<u32> = match dict.get("Length") {
        Some(Object::Integer(l)) => {
            Some(u32::try_from(*l).map_err(|e| ErrMode::from_external_error(buf, e))?)
        }
        Some(Object::Reference(_)) => None,
        _ => {
            (wsc0(), b"endobj".as_slice(), wsc0()).parse_next(buf)?;
            return Ok(Either::Left(Object::Dictionary(dict)));
        }
    };

    let saved_pos = buf.checkpoint();
    match delimited(wsc0(), b"stream".as_slice(), eol2::<_, E>())
        .span()
        .parse_next(buf)
    {
        Ok(range) => {
            if let Some(len) = len {
                (
                    take(len),
                    // pdf file may broken, so we need to repeat till `endstream` and `endobj`
                    repeat_till::<_, _, (), _, _, _, _>(
                        0..,
                        any,
                        (
                            // in pdf.js/test/pdfs/issue10004.pdf.link, use 'endstrea' instead of
                            // 'endstream'
                            wsc0(),
                            b"endstrea".as_slice(),
                            opt(b'm'),
                            wsc0(),
                            b"endobj".as_slice(),
                            wsc0(),
                        ),
                    ),
                )
                    .parse_next(buf)?;
            }
            let bufpos = BufPos::new(
                range
                    .end
                    .try_into()
                    .map_err(|e| ErrMode::from_external_error(buf, e))?,
                len,
            );
            Ok(Either::Right((dict, bufpos)))
        }
        Err(_) => {
            buf.reset(&saved_pos);
            (wsc0(), b"endobj".as_slice(), wsc0()).parse_next(buf)?;
            Ok(Either::Left(Object::Dictionary(dict)))
        }
    }
}

#[cfg(test)]
mod tests;
