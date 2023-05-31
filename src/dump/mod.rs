use hex::encode_upper;
use std::{
    convert::From,
    fmt::{Display, Write},
    str::from_utf8,
};

/// Dump `[u8]` as utf8 str, and hex content
pub struct BinDumper<'a>(pub &'a [u8]);

impl<'a> Display for BinDumper<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_char('"')?;
        f.write_str(from_utf8(self.0).unwrap())?;
        f.write_char('"')?;
        f.write_char(' ')?;
        f.write_fmt(format_args!("({})", &encode_upper(self.0)))
    }
}

/// StrOrString is a wrapper of &str and String, it is used to represent
/// result that borrow exist data or format to a new String instance.
/// Call `as_str` to get &str.
pub enum StrOrString<'a> {
    Str(&'a str),
    String(String),
}

impl<'a> StrOrString<'a> {
    pub fn as_str(&self) -> &'a str {
        todo!()
    }
}

/// Convert [u8] to StrOrString, if it is valid UTF-8, then return Str,
/// otherwise return String in hex format.
impl<'a> From<&'a [u8]> for StrOrString<'a> {
    fn from(s: &'a [u8]) -> Self {
        todo!()
    }
}

/// Convert [u8] to StrOrString, if it is valid UTF-8, then return Str,
/// otherwise return String in hex format.
pub fn u8slice_to_str(s: &[u8]) -> StrOrString {
    s.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bin_dumper() {
        let data = b"hello world";
        let dumper = BinDumper(data);
        assert_eq!(
            format!("{}", dumper),
            "\"hello world\" (68656C6C6F20776F726C64)"
        );
    }
}
