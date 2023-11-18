use super::NOTDEF;
use crate::{
    machine::{Array, Machine, Value},
    parser::header, Encoding256,
};
use anyhow::Result as AnyResult;
use std::{array::from_fn, borrow::Cow};
use winnow::{binary::le_u32, combinator::preceded, error::ContextError, token::any, Parser};

#[derive(Debug, PartialEq)]
pub struct Header {
    /// Type font specification version
    pub spec_ver: String,
    pub font_name: String,
    pub font_ver: String,
}

#[derive(Debug, PartialEq)]
struct EncodingVec(pub [Option<String>; 256]);

#[derive(Debug, PartialEq, Copy, Clone, Eq)]
pub enum PredefinedEncoding {
    Standard,
}

#[derive(Debug, PartialEq)]
enum Encoding {
    Predefined(PredefinedEncoding),
    Vec(Box<EncodingVec>),
}

impl From<Encoding> for Encoding256<'static> {
    fn from(encoding: Encoding) -> Self {
        todo!()
        // match encoding {
        //     Encoding::Predefined(PredefinedEncoding::Standard) => Encoding256::STANDARD,
        //     Encoding::Vec(arr) => {
        //         let arr = (*arr).0;
        //         let mut encoding: [String; 256] = from_fn(|_| NOTDEF.to_owned());
        //         for (i, v) in arr.into_iter().enumerate() {
        //             if let Some(name) = v {
        //                 encoding[i] = name;
        //             }
        //         }
        //         Self::owned(encoding)
        //     }
        // }
    }
}

#[derive(Debug, PartialEq)]
pub struct Font {
    header: Header,
    encoding: Option<Encoding256<'static>>,
}

fn parse_header(mut data: &[u8]) -> AnyResult<Header> {
    match header.parse_next(&mut data) {
        Ok(header) => Ok(header),
        Err(e) => Err(anyhow::anyhow!("Failed to parse header: {}", e)),
    }
}

fn parse_vec_encoding(arr: &Array) -> AnyResult<Encoding> {
    let mut encoding: [Option<String>; 256] = std::array::from_fn(|_| None);
    for (i, v) in arr.iter().enumerate() {
        encoding[i] = v.opt_name().map(|n| (*n).to_owned())
    }
    Ok(Encoding::Vec(Box::new(EncodingVec(encoding))))
}

impl Font {
    pub fn parse(data: &[u8]) -> AnyResult<Self> {
        let data = normalize_pfb(data);
        let header = parse_header(&data)?;
        assert!(header.spec_ver.starts_with("1."), "Not Type1 font");

        let mut machine = Machine::new(&data);
        let encoding = machine.execute_for_encoding()?;
        let encoding = match encoding {
            Value::Array(arr) => parse_vec_encoding(&arr.borrow()),
            Value::PredefinedEncoding(encoding) => Ok(Encoding::Predefined(encoding)),
            _ => anyhow::bail!("Invalid encoding type"),
        }?;

        Ok(Font {
            header,
            encoding: Some(encoding.into()),
        })
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn encoding(&self) -> Option<&Encoding256<'static>> {
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
mod tests {
    use super::*;
    use test_log::test;

    #[test]
    fn parse_pfb_file() {
        let data = include_bytes!("../../nipdf/fonts/d050000l.pfb");
        let font = Font::parse(data).unwrap();
        assert_eq!("Dingbats", font.header.font_name);
    }

    #[test]
    fn parse_pfa_file() {
        let data = include_bytes!("p052024l.pfa");
        let font = Font::parse(data).unwrap();
        assert_eq!("URWPalladioL-BoldItal", font.header.font_name);
    }

    #[test]
    fn parse_std_14_fonts_file() {
        let files: [&[u8]; 14] = [
            include_bytes!("../../nipdf/fonts/d050000l.pfb"),
            include_bytes!("../../nipdf/fonts/n019003l.pfb"),
            include_bytes!("../../nipdf/fonts/n019004l.pfb"),
            include_bytes!("../../nipdf/fonts/n019023l.pfb"),
            include_bytes!("../../nipdf/fonts/n019024l.pfb"),
            include_bytes!("../../nipdf/fonts/n021003l.pfb"),
            include_bytes!("../../nipdf/fonts/n021004l.pfb"),
            include_bytes!("../../nipdf/fonts/n021023l.pfb"),
            include_bytes!("../../nipdf/fonts/n021024l.pfb"),
            include_bytes!("../../nipdf/fonts/n022003l.pfb"),
            include_bytes!("../../nipdf/fonts/n022004l.pfb"),
            include_bytes!("../../nipdf/fonts/n022023l.pfb"),
            include_bytes!("../../nipdf/fonts/n022024l.pfb"),
            include_bytes!("../../nipdf/fonts/s050000l.pfb"),
        ];
        let file_names: [&str; 14] = [
            "Dingbats",
            "NimbusSanL-Regu",
            "NimbusSanL-Bold",
            "NimbusSanL-ReguItal",
            "NimbusSanL-BoldItal",
            "NimbusRomNo9L-Regu",
            "NimbusRomNo9L-Medi",
            "NimbusRomNo9L-ReguItal",
            "NimbusRomNo9L-MediItal",
            "NimbusMonL-Regu",
            "NimbusMonL-Bold",
            "NimbusMonL-ReguObli",
            "NimbusMonL-BoldObli",
            "StandardSymL",
        ];
        for (f, name) in files.into_iter().zip(file_names) {
            let font = Font::parse(f).unwrap();
            assert_eq!(name, font.header.font_name);
        }
    }
}
