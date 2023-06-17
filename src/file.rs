//! Contains types of PDF file structures.

use std::borrow::Cow;

use crate::{
    object::{Dictionary, Entry, Name, Object, ObjectId, ObjectValueError, Section},
    parser::{parse_object, unwrap_parse_result, ParseError},
};

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
pub struct Catalog<'a> {
    dict: Dictionary<'a>,
}

impl<'a> Catalog<'a> {
    pub fn new(dict: Dictionary<'a>) -> Self {
        Self { dict }
    }

    pub fn ver(&self) -> Result<Option<Cow<[u8]>>, ObjectValueError> {
        self.dict
            .get(&Name::new(b"/Version".as_slice()))
            .map(|o| o.as_name())
            .transpose()
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

    pub fn total_objects(&self) -> Result<i32, ObjectValueError> {
        self.dict
            .get(&Name::new(b"/Size".as_slice()))
            .ok_or(ObjectValueError::DictNameMissing)
            .and_then(|obj| obj.as_int())
    }
}

#[derive(Debug, Clone, PartialEq)]
/// Frame contains things like xref, trailer, caused by incremental update. See [FrameSet]
pub struct Frame<'a> {
    xref_pos: u32,
    trailer: Trailer<'a>,
    xref_table: Section,
}

impl<'a> Frame<'a> {
    pub fn new(xref_pos: u32, trailer: Trailer<'a>, xref_table: Section) -> Self {
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

    pub fn total_objects(&self) -> Result<i32, ObjectValueError> {
        self.trailer.total_objects()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FrameSet<'a>(Vec<Frame<'a>>);

impl<'a> FrameSet<'a> {
    pub fn new(frames: Vec<Frame<'a>>) -> Self {
        Self(frames)
    }

    pub fn resolve_object(&self, id: u32) -> Option<Entry> {
        self.iter_entry_by_id(id).next()
    }

    pub fn iter_entry_by_id(&self, id: u32) -> impl Iterator<Item = Entry> + '_ {
        self.0
            .iter()
            .flat_map(move |frame| frame.xref_table.get(&id))
            .copied()
    }

    /// Return total number of objects from the last frame
    pub fn total_objects(&self) -> Result<i32, ObjectValueError> {
        self.0
            .first()
            .ok_or(ObjectValueError::DictNameMissing)
            .and_then(|frame| frame.total_objects())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct File<'a> {
    content: &'a [u8],
    head: Header<'a>,
    frame_set: FrameSet<'a>,
}

impl<'a> File<'a> {
    pub fn new(content: &'a [u8], head: Header<'a>, frame_set: FrameSet<'a>) -> Self {
        Self {
            content,
            head,
            frame_set,
        }
    }

    /// resolve object by id, use newest generation
    pub fn resolve(&self, id: u32) -> Result<Option<Object<'a>>, ParseError<'a>> {
        self.frame_set
            .resolve_object(id)
            .map(|entry| {
                unwrap_parse_result(parse_object(&self.content[entry.offset() as usize..]))
            })
            .transpose()
    }
}

#[cfg(test)]
mod tests;
