pub(crate) mod machine;
pub(crate) mod parser;

pub mod cmap;
mod encoding;
mod pdf_fn;
mod type1;
pub use encoding::Encoding;
pub use pdf_fn::PdfFunc;
pub use type1::Font;

/// PostScript Name Value
pub type Name = kstring::KStringBase<Box<str>>;

/// Create Name from `&str`
#[inline]
#[must_use]
pub fn name(s: &str) -> Name {
    Name::from_ref(s)
}

#[inline]
#[must_use]
pub const fn sname(s: &'static str) -> Name {
    Name::from_static(s)
}

/// Symbol for .notdef glyph
pub const NOTDEF: &str = ".notdef";
