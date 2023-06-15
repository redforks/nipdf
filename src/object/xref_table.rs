use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Copy)]
pub struct XRefEntry(u32, u16, bool); // offset, generation, is_used

impl XRefEntry {
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
    entries: BTreeMap<u32, XRefEntry>,
}

impl Section {
    pub fn new(entries: BTreeMap<u32, XRefEntry>) -> Self {
        Self { entries }
    }
}

impl std::ops::Deref for Section {
    type Target = BTreeMap<u32, XRefEntry>;

    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct XRefTable(Vec<Section>);

impl XRefTable {
    /// first section should be newest
    pub fn new(sections: Vec<Section>) -> Self {
        Self(sections)
    }

    pub fn resolve(&self, id: u32) -> Option<XRefEntry> {
        self.iter_entry_by_id(id).next()
    }

    pub fn iter_entry_by_id(&self, id: u32) -> impl Iterator<Item = XRefEntry> + '_ {
        self.0
            .iter()
            .flat_map(move |section| section.get(&id))
            .copied()
    }
}

#[cfg(test)]
mod tests;
