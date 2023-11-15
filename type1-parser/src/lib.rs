pub(crate) mod machine;
pub(crate) mod parser;

use machine::{Array, Machine};
use parser::header;
use winnow::{binary::le_u32, combinator::preceded, error::ContextError, token::any, Parser};

type AnyResult<T> = Result<T, anyhow::Error>;

#[derive(Debug, PartialEq)]
pub struct Header {
    /// Type font specification version
    pub spec_ver: String,
    pub font_name: String,
    pub font_ver: String,
}

#[derive(Debug, PartialEq)]
pub struct Encoding(pub [Option<String>; 256]);

#[derive(Debug, PartialEq)]
pub struct Font {
    header: Header,
    encoding: Option<Encoding>,
}

fn parse_header(mut data: &[u8]) -> AnyResult<Header> {
    match header.parse_next(&mut data) {
        Ok(header) => Ok(header),
        Err(e) => Err(anyhow::anyhow!("Failed to parse header: {}", e)),
    }
}

fn parse_encoding(arr: &Array) -> AnyResult<Encoding> {
    let mut encoding: [Option<String>; 256] = std::array::from_fn(|_| None);
    for (i, v) in arr.iter().enumerate() {
        encoding[i] = v.opt_name().map(|n| (*n).to_owned())
    }
    Ok(Encoding(encoding))
}

impl Font {
    pub fn parse(data: impl Into<Vec<u8>>) -> AnyResult<Self> {
        let data = normalize_pfb(data.into());
        let header = parse_header(&data)?;
        assert!(header.spec_ver.starts_with("1."), "Not Type1 font");

        let mut machine = Machine::new(data);
        machine.execute()?;
        let fonts = machine.take_fonts();
        assert_eq!(fonts.len(), 1);
        let font = fonts.into_iter().next().unwrap();
        let encoding = font
            .1
            .get("Encoding")
            .map(|v| parse_encoding(&v.array()?.borrow()))
            .transpose()?;

        Ok(Font { header, encoding })
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn encoding(&self) -> Option<&Encoding> {
        self.encoding.as_ref()
    }
}

/// If file is pfb file, remove pfb section bytes
fn normalize_pfb(mut data: Vec<u8>) -> Vec<u8> {
    if data.len() < 100 || data[0] != 0x80 {
        return data;
    }

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

    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pfb_file() {
        let data = include_bytes!("../../nipdf/fonts/d050000l.pfb");
        let font = Font::parse(*data).unwrap();
        assert_eq!("Dingbats", font.header.font_name);
    }
}
