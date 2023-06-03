use std::fmt::{Display, Write};

use lopdf::Object;

pub mod object;
pub mod objects;
pub mod query;
pub mod xref;

#[derive(Clone, Copy, PartialEq, Debug)]
/// When display, render n*2 spaces
struct Indent(usize);

impl Indent {
    fn inc(self) -> Self {
        Self(self.0 + 1)
    }
}

impl Display for Indent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for _ in 0..(self.0 * 2) {
            f.write_char(' ')?;
        }
        Ok(())
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default, strum::EnumString)]
pub enum ObjectType {
    #[default]
    Stream,
    Other,
}

impl From<&Object> for ObjectType {
    fn from(o: &Object) -> Self {
        match o {
            Object::Stream(_) => Self::Stream,
            _ => Self::Other,
        }
    }
}

#[cfg(test)]
mod tests;
