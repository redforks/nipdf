use crate::{
    AnyWhatever, Encoding, Result,
    machine::{Array, Machine, Value},
    parser::header,
    sname,
};
use snafu::{FromString as _, OptionExt as _, ResultExt as _, ensure_whatever, whatever};
use std::{array::from_fn, borrow::Cow};
use winnow::{
    Parser,
    binary::le_u32,
    combinator::{preceded, terminated},
    token::{any, rest},
};

#[derive(Debug, PartialEq)]
pub struct Header {
    /// Type font specification version
    pub spec_ver: String,
    pub font_name: String,
    pub font_ver: String,
}

#[derive(Debug, PartialEq)]
pub struct Font {
    header: Header,
    encoding: Option<Encoding>,
}

fn parse_header(data: &[u8]) -> Result<Header> {
    terminated(header::<crate::ParserError>, rest)
        .parse(data)
        .map_err(winnow::error::ParseError::into_inner)
        .whatever_context("parse header")
}

fn parse_vec_encoding(arr: &Array) -> Result<Encoding> {
    let mut names = from_fn(|_| sname(".notdef"));
    for (i, v) in arr.iter().enumerate() {
        names[i] = v.name().whatever_context("get encoding name")?;
    }
    Ok(Encoding::new(names))
}

impl Font {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let data = normalize_pfb(data)?;
        let header = parse_header(&data)?;
        ensure_whatever!(header.spec_ver.starts_with("1."), "Not Type1 font");

        let mut machine = Machine::new(&data);
        let encoding = machine
            .execute_for_encoding()
            .whatever_context("execute for encoding")?;
        let encoding = match encoding {
            Value::Array(arr) => {
                parse_vec_encoding(&arr.borrow()).whatever_context("parse encodings")?
            }
            Value::PredefinedEncoding(encoding) => {
                Encoding::predefined(&encoding).whatever_context("get predefined encoding")?
            }
            _ => whatever!("Invalid encoding type"),
        };

        Ok(Font {
            header,
            encoding: Some(encoding),
        })
    }

    #[inline]
    pub fn header(&self) -> &Header {
        &self.header
    }

    #[inline]
    pub fn encoding(&self) -> Option<&Encoding> {
        self.encoding.as_ref()
    }

    #[inline]
    pub fn take_encoding(self) -> Option<Encoding> {
        self.encoding
    }
}

/// If file is pfb file, remove pfb section bytes
fn normalize_pfb(data: &[u8]) -> Result<Cow<'_, [u8]>> {
    if data.len() < 100 || data[0] != 0x80 {
        return Ok(Cow::Borrowed(data));
    }

    let mut data = data.to_vec();
    let mut pos = 0;
    for _ in 0..3 {
        let section_len = preceded((0x80u8, any::<_, crate::ParserError>), le_u32)
            .parse(&data[pos..(6 + pos)])
            .map_err(|e| {
                AnyWhatever::with_source(Box::new(e.into_inner()), "get pfb section len".into())
            })? as usize;
        data.drain(pos..(pos + 6));
        pos += section_len;
    }

    Parser::<_, _, crate::ParserError>::parse(&mut &b"\x80\x03"[..], &data[pos..])
        .map_err(|e| AnyWhatever::with_source(Box::new(e.into_inner()), "skip pfb tag".into()))?;
    data.drain(pos..);

    Ok(data.into())
}

#[cfg(test)]
mod tests;
