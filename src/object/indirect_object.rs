use crate::parser::{parse_complete_object, ParseError};

use super::Object;
use once_cell::unsync::OnceCell;
use std::result::Result;

pub struct IndirectObject<'a> {
    id: u32,
    generation: u16,
    content: &'a [u8],
    resolved: OnceCell<Object<'a>>,
}

impl<'a> IndirectObject<'a> {
    pub fn new(id: u32, generation: u16, content: &'a [u8]) -> Self {
        Self {
            id,
            generation,
            content,
            resolved: OnceCell::new(),
        }
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn generation(&self) -> u16 {
        self.generation
    }

    pub fn object(&self) -> Result<&Object<'a>, ParseError> {
        self.resolved
            .get_or_try_init(|| parse_complete_object(self.content))
    }
}

#[cfg(test)]
mod tests;
