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
    // UTF-16 compatible to UCS2
    "UniGB-UCS2-H" => encoding_rs::UTF_16BE,
    "UniGB-UCS2-V" => encoding_rs::UTF_16BE,
    "UniGB-UTF16-H" => encoding_rs::UTF_16BE,
    "UniGB-UTF16-V" => encoding_rs::UTF_16BE,

    "B5pc-H" => encoding_rs::BIG5,
    "B5pc-V" => encoding_rs::BIG5,
    // Hong Kong SCS, an extension to the Big Five character set and encoding
    "HKscs-B5-H" => encoding_rs::BIG5,
    "HKscs-B5-V" => encoding_rs::BIG5,
    // ETen-B5: big5 with ETen, currently big5 implicit has ETen extension
    "ETen-B5-H" => encoding_rs::BIG5,
    "ETen-B5-V" => encoding_rs::BIG5,
    // Same as ETen-B5, but replace half-width Latin characters with proportional forms
    "ETenms-B5-H" => encoding_rs::BIG5,
    "ETenms-B5-V" => encoding_rs::BIG5,
    // TODO: CNS-EUC-H/CNS-EUC-V, encoding_rs no EUC-TW yet
    "UniCNS-UCS2-H" => encoding_rs::UTF_16BE,
    "UniCNS-UCS2-V" => encoding_rs::UTF_16BE,
    "UniCNS-UTF16-H" => encoding_rs::UTF_16BE,
    "UniCNS-UTF16-V" => encoding_rs::UTF_16BE,

    "83pv-RKSJ-H" => encoding_rs::SHIFT_JIS,
    "90ms-RKSJ-H" => encoding_rs::SHIFT_JIS,
    "90ms-RKSJ-V" => encoding_rs::SHIFT_JIS,
    "90msp-RKSJ-H" => encoding_rs::SHIFT_JIS,
    "90msp-RKSJ-V" => encoding_rs::SHIFT_JIS,
    "90pv-RKSJ-H" => encoding_rs::SHIFT_JIS,
    "Add-RKSJ-H" => encoding_rs::SHIFT_JIS,
    "Add-RKSJ-V" => encoding_rs::SHIFT_JIS,
    "EUC-H" => encoding_rs::EUC_JP,
    "EUC-V" => encoding_rs::EUC_JP,
    "Ext-RKSJ-H" => encoding_rs::SHIFT_JIS,
    "Ext-RKSJ-V" => encoding_rs::SHIFT_JIS,
    "H" => encoding_rs::EUC_JP,
    "V" => encoding_rs::EUC_JP,
    "UniJIS-UCS2-H" => encoding_rs::UTF_16BE,
    "UniJIS-UCS2-V" => encoding_rs::UTF_16BE,
    "UniJIS-UCS2-HW-H" => encoding_rs::UTF_16BE,
    "UniJIS-UCS2-HW-V" => encoding_rs::UTF_16BE,
    "UniJIS-UTF16-H" => encoding_rs::UTF_16BE,
    "UniJIS-UTF16-V" => encoding_rs::UTF_16BE,

    "KSC-EUC-H" => encoding_rs::EUC_KR,
    "KSC-EUC-V" => encoding_rs::EUC_KR,
    "KSCms-UHC-H" => encoding_rs::EUC_KR,
    "KSCms-UHC-V" => encoding_rs::EUC_KR,
    "KSCpc-EUC-H" => encoding_rs::EUC_KR,
    "KSCpc-EUC-V" => encoding_rs::EUC_KR,
    "UniKS-UTF16-H" => encoding_rs::UTF_16BE,
    "UniKS-UTF16-V" => encoding_rs::UTF_16BE,
};

#[cfg(test)]
mod tests;
