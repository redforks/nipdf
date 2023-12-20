//! CMap used in Type0/CID Fonts

use anyhow::Result as AnyResult;
use encoding_rs::Encoding;
use phf::phf_map;

/// CMap used in Type0/CID Fonts.
/// Convert Char code to CID.
///
/// Normally CID is unicode char, unless a cmap file used.
pub struct CMap {
    // None to use identity mapping
    encoding: Option<&'static Encoding>,
}

impl CMap {
    pub fn predefined(name: &str) -> AnyResult<Self> {
        // Identity-H & Identity-V is not UTF-16, UTF-16 is variable-length encoding
        if name == "Identity-H" || name == "Identity-V" {
            return Ok(Self { encoding: None });
        }

        CMAP_TO_ENCODING
            .get(name)
            .map(|e| Self { encoding: Some(e) })
            .ok_or_else(|| anyhow::anyhow!("unknown cmap name: {}", name))
    }

    pub fn decode(&self, data: &[u8]) -> Vec<u32> {
        self.encoding.map_or_else(
            || {
                debug_assert!(data.len() % 2 == 0, "{:?}", data);
                let mut rv = Vec::with_capacity(data.len() / 2);
                for i in 0..data.len() / 2 {
                    let ch = u16::from_be_bytes([data[i * 2], data[i * 2 + 1]]);
                    rv.push(ch as u32);
                }
                rv
            },
            |encoding| {
                let (s, detected_encoding, has_wrong_char) = encoding.decode(data);
                if detected_encoding != encoding || has_wrong_char {
                    log::warn!(
                        "cmap decode: detected encoding: {:?}, has wrong char: {}",
                        detected_encoding,
                        has_wrong_char
                    );
                }
                s.chars().map(|c| c as u32).collect()
            },
        )
    }
}

static CMAP_TO_ENCODING: phf::Map<&'static str, &'static Encoding> = phf_map! {
    // should be GB2312, GBK is compatible to GB2312, `encoding-rs` no GB2312
    // GB2312-80 in Windows
    "GB-EUC-H" => encoding_rs::GBK,
    "GB-EUC-V" => encoding_rs::GBK,
    // GB2312-80 in Mac OS
    "GBpc-EUC-H" => encoding_rs::GBK,
    "GBpc-EUC-V" => encoding_rs::GBK,
    "GBK-EUC-H" => encoding_rs::GBK,
    "GBK-EUC-V" => encoding_rs::GBK,
    // GBK but replaces half-width Latin characters with proportional
    // forms and maps character code 0x24 to a dollar sign($) instead of a yuan symbol(¥)
    "GBKp-EUC-H" => encoding_rs::GBK,
    "GBKp-EUC-V" => encoding_rs::GBK,
    "GBK2K-H" => encoding_rs::GB18030,
    "GBK2K-V" => encoding_rs::GB18030,

    // ETen-B5: big5 with ETen, currently big5 implicit has ETen extension
    "ETen-B5-H" => encoding_rs::BIG5,
    "ETen-B5-V" => encoding_rs::BIG5,
};

#[cfg(test)]
mod tests;
