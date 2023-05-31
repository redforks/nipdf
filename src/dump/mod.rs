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
