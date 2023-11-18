pub(crate) mod machine;
pub(crate) mod parser;

mod encoding;
mod type1;
pub use encoding::Encoding;
use string_interner::{backend::BucketBackend, symbol::SymbolU16, StringInterner};
pub use type1::Font;

/// Symbol for .notdef glyph
pub const NOTDEF: &str = ".notdef";
pub type Name = SymbolU16;
pub type NameRegistry = StringInterner<BucketBackend<Name>>;
