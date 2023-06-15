use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Copy)]
pub struct XRefEntry(u32, u16, bool); // offset, generation, is_free

impl XRefEntry {
    pub fn new(offset: u32, generation: u16, is_free: bool) -> Self {
        Self(offset, generation, is_free)
    }

    pub fn offset(&self) -> u32 {
        self.0
    }

    pub fn generation(&self) -> u16 {
        self.1
    }

    pub fn is_free(&self) -> bool {
        self.2
    }
}

#[derive(Debug)]
pub struct XRefTable {
    entries: BTreeMap<u32, XRefEntry>,
}

impl XRefTable {
    pub fn new(entries: BTreeMap<u32, XRefEntry>) -> Self {
        Self { entries }
    }
}

impl std::ops::Deref for XRefTable {
    type Target = BTreeMap<u32, XRefEntry>;

    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}
