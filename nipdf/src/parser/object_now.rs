use super::{eol2, eol3, ws_prefixed1, wsc_prefixed0};
use crate::{
    object::{
        BufPos, Dictionary, HexString, IndirectObjectDef, InnerString, LiteralString, Object,
        ObjectId, ObjectValueError, Reference, Stream as PdfStream,
    },
    parser::is_whitespace,
};
use ahash::HashMap;
use either::Either;
use hex::FromHexError;
use nom::AsBytes;
use prescript::Name;
use std::{
    borrow::Cow,
    num::{NonZeroU32, ParseIntError},
};
use winnow::{
    PResult, Parser,
    ascii::{Caseless, dec_uint, float},
    combinator::{alt, delimited, preceded, repeat, rest, terminated},
    error::FromExternalError,
    stream::{AsBStr, AsChar, Compare, ContainsToken, Location, Stream, StreamIsPartial},
    token::{any, take_till, take_while},
};

fn name<'a, S, E>() -> impl Parser<S, Name, E> + 'a
where
    S: Stream<Token = u8, Slice = &'a [u8]> + StreamIsPartial + Compare<u8> + 'a,
    E: winnow::error::ParserError<S> + 'a,
    E: FromExternalError<S, ObjectValueError>,
{
    preceded(
        b'/',
        take_till(0.., &[
            b' ', b'\t', b'\r', b'\n', b'\x0C', b'[', b'<', b'(', b'/', b'>', b']',
        ]),
    )
    .try_map(normalize_name)
}

/// Return `Err(ObjectValueError::InvalidNameFormat)` if the name is not a valid PDF name encoding,
/// not two hex char after `#`.
fn normalize_name(buf: &[u8]) -> Result<Name, ObjectValueError> {
    fn next_hex_char(iter: &mut impl Iterator<Item = u8>) -> Option<u8> {
        let mut result = 0;
        for _ in 0..2 {
            if let Some(c) = iter.next() {
                result <<= 4;
                result |= match c {
                    b'0'..=b'9' => c - b'0',
                    b'a'..=b'f' => c - b'a' + 10,
                    b'A'..=b'F' => c - b'A' + 10,
                    _ => return None,
                };
            } else {
                return None;
            }
        }
        Some(result)
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

fn number<'a, S, E>() -> impl Parser<S, Object, E> + 'a
where
    S: Stream<Token = u8, Slice = &'a [u8]>
        + StreamIsPartial
        + Compare<u8>
        + Compare<char>
        + AsBStr
        + Compare<Caseless<&'static str>>
        + 'a,
    <S as Stream>::IterOffsets: Clone,
    E: winnow::error::ParserError<S> + 'a,
    E: winnow::error::ParserError<&'a [u8]> + 'a,
{
    fn fallback<E>(buf: &mut &[u8]) -> PResult<Object, E> {
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

fn parse_quoted_string<'a, S, E>(input: &mut S) -> PResult<LiteralString, E>
where
    S: Stream<Token = u8, Slice = &'a [u8]> + StreamIsPartial + Compare<u8> + 'a,
    E: winnow::error::ParserError<S> + 'a,
    E: FromExternalError<S, ParseIntError>,
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
    hex::decode(&buf).map(|v| HexString(v.as_bytes().into()))
}

fn hex_string<'a, S, E>() -> impl Parser<S, Object, E> + 'a
where
    S: Stream<Token = u8, Slice = &'a [u8]> + StreamIsPartial + Compare<u8> + 'a,
    E: winnow::error::ParserError<S> + FromExternalError<S, FromHexError> + 'a,
{
    let parser = take_while(
        ..,
        (AsChar::is_hex_digit, [b' ', b'\t', b'\r', b'\n', b'\x0C']),
    );
    delimited(b'<', parser.try_map(decode_hex), b'>').map(Object::HexString)
}

fn array<'a, S, E>(input: &mut S) -> PResult<Object, E>
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
    E: winnow::error::ParserError<S> + 'a,
    E: FromExternalError<S, ObjectValueError>,
    E: winnow::error::ParserError<&'a [u8]> + 'a,
    E: FromExternalError<S, FromHexError> + 'a,
    E: FromExternalError<S, ParseIntError> + 'a,
{
    let item = repeat::<_, _, Vec<_>, _, _>(0.., wsc_prefixed0(object()));
    delimited(b'[', item, wsc_prefixed0(b']'))
        .output_into()
        .parse_next(input)
}

pub(crate) fn dict<'a, S, E>(input: &mut S) -> PResult<Dictionary, E>
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
    E: winnow::error::ParserError<S> + 'a,
    E: FromExternalError<S, ObjectValueError>,
    E: winnow::error::ParserError<&'a [u8]> + 'a,
    E: FromExternalError<S, FromHexError> + 'a,
    E: FromExternalError<S, ParseIntError> + 'a,
{
    let key = wsc_prefixed0(name());
    let value = wsc_prefixed0(object());
    let pair = repeat::<_, _, HashMap<_, _>, _, _>(0.., (key, value)).map(Dictionary::from);
    delimited(b"<<".as_slice(), pair, wsc_prefixed0(b">>".as_slice())).parse_next(input)
}

fn object_id<'a, S, E>() -> impl Parser<S, ObjectId, E> + 'a
where
    S: Stream<Token = u8, Slice = &'a [u8]> + StreamIsPartial + Compare<u8> + 'a,
    E: winnow::error::ParserError<S> + 'a,
{
    (dec_uint, ws_prefixed1(dec_uint)).map(|(id, gen): (u32, u16)| ObjectId::new(id, gen))
}

fn reference<'a, S, E>() -> impl Parser<S, Object, E> + 'a
where
    S: Stream<Token = u8, Slice = &'a [u8]> + StreamIsPartial + Compare<u8> + 'a,
    E: winnow::error::ParserError<S> + 'a,
{
    terminated(object_id(), ws_prefixed1(b'R'))
        .map(|id| Reference::new(id.id(), id.generation()).into())
}

/// Return parser to parse [Object].
fn object<'a, S, E>() -> impl Parser<S, Object, E> + 'a
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
    E: winnow::error::ParserError<S> + 'a,
    E: FromExternalError<S, ObjectValueError>,
    E: winnow::error::ParserError<&'a [u8]> + 'a,
    E: FromExternalError<S, FromHexError> + 'a,
    E: FromExternalError<S, ParseIntError>,
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

/// Return parser to parse indirect object definition.
///
/// Because the complexity of stream object, parser not consume all input, it will end at the end of
/// object definition, or at the begin of stream object.
fn indirect_object_def<'a, S, E>() -> impl Parser<S, IndirectObjectDef, E> + 'a
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
    E: FromExternalError<S, ObjectValueError>,
    E: winnow::error::ParserError<&'a [u8]> + 'a,
    E: FromExternalError<S, FromHexError> + 'a,
    E: FromExternalError<S, ParseIntError> + 'a,
{
    (
        object_id(),
        preceded(
            ws_prefixed1(b"obj".as_slice()),
            ws_prefixed1(indirect_object_content),
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
) -> PResult<Either<Object, (Dictionary, BufPos)>, E>
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
    E: FromExternalError<S, ObjectValueError>,
    E: winnow::error::ParserError<&'a [u8]> + 'a,
    E: FromExternalError<S, FromHexError> + 'a,
    E: FromExternalError<S, ParseIntError>,
{
    let o = object().parse_next(buf)?;
    let Object::Dictionary(dict) = o else {
        return Ok(Either::Left(o));
    };
    let len: Option<NonZeroU32> = match dict.get("Length") {
        Some(Object::Integer(l)) => Some(NonZeroU32::try_from(u32::try_from(*l).unwrap()).unwrap()),
        Some(Object::Reference(_)) => None,
        _ => return Ok(Either::Left(Object::Dictionary(dict))),
    };

    let saved_pos = buf.checkpoint();
    match terminated(wsc_prefixed0(b"stream".as_slice()), eol2::<_, E>())
        .span()
        .parse_next(buf)
    {
        Ok(range) => {
            let bufpos = BufPos::new(range.end.try_into().unwrap(), len);
            Ok(Either::Right((dict, bufpos)))
        }
        Err(_) => {
            buf.reset(&saved_pos);
            Ok(Either::Left(Object::Dictionary(dict)))
        }
    }
}

#[cfg(test)]
mod tests;
