//! Efficient way to store PostScript Name Value

/// PostScript Name Value
pub type Name = kstring::KStringBase<Box<str>>;

/// Special name to match normally won't exist.
pub static INVALID1: Name = Name::from_static("$$invalid1$$");
/// Special name to match normally won't exist.
pub static INVALID2: Name = Name::from_static("$$invalid2$$");

/// Create Name from `&str`
pub fn name(s: &str) -> Name {
    Name::from_ref(s)
}
