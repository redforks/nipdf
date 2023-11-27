pub(crate) mod machine;
pub(crate) mod name;
pub(crate) mod parser;

mod encoding;
mod type1;
pub use encoding::Encoding;
pub use name::{name, Name, INVALID1, INVALID2};
pub use type1::Font;

#[macro_export]
macro_rules! name {
    ($s:literal) => {
        $crate::Name::from_static($s)
    };
}

/// Symbol for .notdef glyph
pub const NOTDEF: &str = ".notdef";
