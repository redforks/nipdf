use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Copy)]
pub struct Entry(u32, u16, bool); // offset, generation, is_used

impl Entry {
    pub fn new(offset: u32, generation: u16, is_used: bool) -> Self {
        Self(offset, generation, is_used)
    }

    pub fn offset(&self) -> u32 {
        self.0
    }

    pub fn generation(&self) -> u16 {
        self.1
    }

    pub fn is_used(&self) -> bool {
        self.2
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct Section {
    entries: BTreeMap<u32, Entry>,
}

impl Section {
    pub fn new(entries: BTreeMap<u32, Entry>) -> Self {
        Self { entries }
    }
}

impl std::ops::Deref for Section {
    type Target = BTreeMap<u32, Entry>;

    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}
