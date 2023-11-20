pub mod __private;
pub(crate) mod machine;
pub(crate) mod name;
pub(crate) mod parser;

mod encoding;
mod type1;
pub use encoding::Encoding;
pub use name::{name, Name as Name2, INVALID1, INVALID2};
use string_interner::{backend::BucketBackend, symbol::SymbolU16, StringInterner};
pub use type1::Font;

/// Symbol for .notdef glyph
pub const NOTDEF: &str = ".notdef";
pub type Name = SymbolU16;
pub type NameRegistry = StringInterner<BucketBackend<Name>>;
