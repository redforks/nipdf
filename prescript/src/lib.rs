pub(crate) mod machine;
pub(crate) mod parser;

mod encoding;
mod type1;
pub use encoding::Encoding;
pub use type1::Font;

/// PostScript Name Value
pub type Name = kstring::KStringBase<Box<str>>;

/// Create Name from `&str`
pub fn name(s: &str) -> Name {
    Name::from_ref(s)
}

pub const fn sname(s: &'static str) -> Name {
    Name::from_static(s)
}

/// Symbol for .notdef glyph
pub const NOTDEF: &str = ".notdef";
