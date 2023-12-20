//! CMap used in Type0/CID Fonts

use anyhow::Result as AnyResult;
use encoding_rs::Encoding;

/// CMap used in Type0/CID Fonts.
/// Convert Char code to CID.
///
/// Normally CID is unicode char, unless a cmap file used.
pub struct CMap {
    encoding: Encoding,
}

impl CMap {
    pub fn predefined(name: &str) -> AnyResult<Self> {
        todo!()
    }

    pub fn decode(&self, data: &[u8]) -> Vec<u32> {
        todo!()
    }
}
