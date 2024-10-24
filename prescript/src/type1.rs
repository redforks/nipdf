use crate::{
    Encoding,
    machine::{Array, Machine, Value},
    parser::header,
    sname,
};
use snafu::{FromString, Whatever, prelude::*};
use std::{array::from_fn, borrow::Cow};
use winnow::{Parser, binary::le_u32, combinator::preceded, error::ContextError, token::any};

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

fn parse_header(mut data: &[u8]) -> Result<Header, Whatever> {
    header
        .parse_next(&mut data)
        .map_err(|e| Whatever::without_source(format!("Failed to parse header: {}", e)))
}

fn parse_vec_encoding(arr: &Array) -> Encoding {
    let mut names = from_fn(|_| sname(".notdef"));
    for (i, v) in arr.iter().enumerate() {
        names[i] = v.name().unwrap();
    }
    Encoding::new(names)
}

impl Font {
    pub fn parse(data: &[u8]) -> Result<Self, Whatever> {
        let data = normalize_pfb(data);
        let header = parse_header(&data)?;
        assert!(header.spec_ver.starts_with("1."), "Not Type1 font");

        let mut machine = Machine::new(&data);
        let encoding = machine
            .execute_for_encoding()
            .whatever_context("execute for encoding")?;
        let encoding = match encoding {
            Value::Array(arr) => parse_vec_encoding(&arr.borrow()),
            Value::PredefinedEncoding(encoding) => Encoding::predefined(encoding).unwrap(),
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
}

/// If file is pfb file, remove pfb section bytes
fn normalize_pfb(data: &[u8]) -> Cow<[u8]> {
    if data.len() < 100 || data[0] != 0x80 {
        return Cow::Borrowed(data);
    }

    let mut data = data.to_vec();
    let mut pos = 0;
    for _ in 0..3 {
        let section_len = preceded((0x80u8, any), le_u32::<_, ContextError>)
            .parse(&data[pos..(6 + pos)])
            .unwrap() as usize;
        data.drain(pos..(pos + 6));
        pos += section_len;
    }

    Parser::<_, _, ContextError>::parse(&mut &b"\x80\x03"[..], &data[pos..]).unwrap();
    data.drain(pos..);

    data.into()
}

#[cfg(test)]
mod tests;
