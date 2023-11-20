//! Efficient way to store PostScript Name Value
//!
//! PostScript has many frequently used names, such as font glyph names,
//! operation names etc. Use `String`, `Vec<str>`, `Box<str>` needs a lot
//! of allocations, and `Rc<str>` complex code logic.
//!
//! This crate provides a efficient way to store PostScript Name Value.
//! `Name` type wraps a `Either` enum type, which is `Either<u16, Box<str>>`.
//! Predefine common used names, given each name a unique u16 number, and
//! other names stored in Box<str>. Static strings stored in a const array,
//! sorted by alphabetical order, so we can use binary search to find a name.
//!
//! Use `name!()` macro to resolve `&'static str` to Name at compile time,
//! no runtime cost.

use either::Either::{self, Left, Right};

mod built_in_names;
// Hack to use name!() macro
use crate as prescript;
use built_in_names::BUILT_IN_NAMES;
use prescript_macro::name;
use std::fmt::{Display, Formatter};

/// PostScript Name Value
/// Important: Always use name function and macro create Name, don't create
/// Name directly. The internal structure exposed for pattern match combined with `name!` macro:
///
/// match name {
///   name!("foo") => { ... }
///   name!("bar") => { ...}
///   _ => { ... }
/// }  
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Name(pub Either<u16, Box<str>>);

impl std::fmt::Debug for Name {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "name!({})", self.as_ref())
    }
}

/// Special name to match normally won't exist.
pub static INVALID1: Name = name!("$$invalid1$$");
/// Special name to match normally won't exist.
pub static INVALID2: Name = name!("$$invalid2$$");

impl Display for Name {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "/{}", self.as_ref())
    }
}

impl AsRef<str> for Name {
    fn as_ref(&self) -> &str {
        match &self.0 {
            Left(i) => BUILT_IN_NAMES[*i as usize],
            Right(s) => s.as_ref(),
        }
    }
}

/// Create Name from `&str`, if it is one of builtin names, `Name` will
/// use `u16` to store it, otherwise, `Name` will use `Box<str>` to store it.
///
/// Preferred to use `name!()` macro if possible.
pub fn name(s: &str) -> Name {
    // binary search BUILT_IN_NAMES, return its index if found, otherwise return boxed str
    match BUILT_IN_NAMES.binary_search(&s) {
        Ok(i) => Name(Left(i as u16)),
        Err(_) => Name(Right(Box::from(s))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use static_assertions::assert_eq_size;

    #[test]
    fn test_name_size() {
        assert_eq_size!(Name, (usize, usize));
    }

    #[test]
    fn name_from_str() {
        assert!(matches!(name("foo"), Name(Right(_))));
        assert!(matches!(name("for"), Name(Left(_))));
    }

    #[test]
    fn as_str() {
        assert_eq!("foo", name("foo").as_ref());
        assert_eq!("for", name("for").as_ref());
    }

    #[test]
    fn name_macro() {
        assert_eq!(name!("foo"), name("foo"));
        assert_eq!(name!("for"), name("for"));
    }

    #[test]
    fn display() {
        assert_eq!(format!("{}", name!("foo")), "/foo");
        assert_eq!(format!("{}", name!("for")), "/for");
    }
}
