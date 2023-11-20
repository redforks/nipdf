pub mod __private;
pub(crate) mod machine;
pub(crate) mod name;
pub(crate) mod parser;

mod encoding;
mod type1;
pub use encoding::Encoding;
pub use name::{name, Name, INVALID1, INVALID2};
pub use type1::Font;

/// Symbol for .notdef glyph
pub const NOTDEF: &str = ".notdef";
