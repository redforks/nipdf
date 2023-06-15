//! Contains types of PDF file structures.

use crate::object::{Dictionary, Name, Object, XRefEntry, XRefTable};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Header<'a>(&'a [u8]);

impl<'a> Header<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self(buf)
    }

    pub fn ver(&self) -> &str {
        std::str::from_utf8(self.0).unwrap()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Trailer<'a> {
    dict: Dictionary<'a>,
}

impl<'a> Trailer<'a> {
    pub fn new(dict: Dictionary<'a>) -> Self {
        Self { dict }
    }
}

#[derive(Debug, Clone, PartialEq)]
/// Frame contains things like xref, trailer, caused by incremental update. See [FrameSet]
pub struct Frame<'a> {
    xref_pos: u32,
    trailer: Trailer<'a>,
    xref_table: XRefTable,
}

impl<'a> Frame<'a> {
    pub fn new(xref_pos: u32, trailer: Trailer<'a>, xref_table: XRefTable) -> Self {
        Self {
            xref_pos,
            trailer,
            xref_table,
        }
    }

    /// Return the position of the previous frame
    pub fn prev(&self) -> Option<u32> {
        self.trailer
            .dict
            .get(&Name::new(b"/Prev".as_slice()))
            .and_then(|obj| match obj {
                Object::Integer(i) => Some(*i as u32),
                _ => unreachable!(),
            })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FrameSet<'a>(Vec<Frame<'a>>);

impl<'a> FrameSet<'a> {
    pub fn new(frames: Vec<Frame<'a>>) -> Self {
        Self(frames)
    }

    pub fn resolve_object(&self, id: u32) -> Option<XRefEntry> {
        self.iter_entry_by_id(id).next()
    }

    pub fn iter_entry_by_id(&self, id: u32) -> impl Iterator<Item = XRefEntry> + '_ {
        self.0
            .iter()
            .flat_map(move |frame| frame.xref_table.get(&id))
            .copied()
    }
}

#[cfg(test)]
mod tests;
