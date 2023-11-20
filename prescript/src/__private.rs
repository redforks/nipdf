//! Defines private functions used by `name!()` macro
use crate::name::*;
use either::Either;

pub const fn left_name(i: u16) -> Name {
    Name(Either::Left(i))
}

pub fn right_name(s: &str) -> Name {
    Name(Either::Right(s.into()))
}
