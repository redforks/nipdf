pub(crate) mod machine;
pub(crate) mod parser;

mod encoding;
mod type1;
pub use encoding::Encoding256;
pub use type1::Font;

/// Symbol for .notdef glyph
pub const NOTDEF: &str = ".notdef";
