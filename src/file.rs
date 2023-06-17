//! Contains types of PDF file structures.

use std::{borrow::Cow, collections::HashMap, num::NonZeroUsize};

use crate::{
    object::{self, Dictionary, FrameSet, Name, Object, ObjectValueError},
    parser::parse_object,
};
use lru::LruCache;
use nohash_hasher::BuildNoHashHasher;

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

type IDOffsetMap = HashMap<u32, u32, BuildNoHashHasher<u32>>;

pub struct XRefTable<'a> {
    buf: &'a [u8],
    id_offset: IDOffsetMap, // object id -> offset
}

impl<'a> XRefTable<'a> {
    fn new(buf: &'a [u8], id_offset: IDOffsetMap) -> Self {
        Self { buf, id_offset }
    }

    pub fn scan(frame_set: &FrameSet) -> IDOffsetMap {
        let mut r = IDOffsetMap::with_capacity_and_hasher(5000, BuildNoHashHasher::default());
        for (id, entry) in frame_set
            .iter()
            .rev()
            .map(|f| f.xref_section.iter())
            .flatten()
        {
            if entry.is_used() {
                r.insert(*id, entry.offset());
            } else {
                r.remove(id);
            }
        }
        r
    }

    pub fn from_frame_set(buf: &'a [u8], frame_set: &FrameSet) -> Self {
        Self::new(buf, Self::scan(frame_set))
    }

    /// Return `buf` start from where `id` is
    pub fn resolve_object_buf(&self, id: u32) -> Option<&'a [u8]> {
        self.id_offset
            .get(&id)
            .map(|offset| &self.buf[*offset as usize..])
    }
}

pub struct ObjectResolver<'a> {
    xref_table: XRefTable<'a>,
    lru: LruCache<u32, Option<Object<'a>>>,
}

impl<'a> ObjectResolver<'a> {
    pub fn new(xref_table: XRefTable<'a>) -> Self {
        Self {
            xref_table,
            lru: LruCache::new(NonZeroUsize::new(5000).unwrap()),
        }
    }

    pub fn resolve(&mut self, id: u32) -> Option<&Object<'a>> {
        self.lru
            .get_or_insert(id, || {
                self.xref_table
                    .resolve_object_buf(id)
                    .map(|buf| parse_object(buf).unwrap().1)
            })
            .as_ref()
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
pub struct File<'a> {
    content: &'a [u8],
    head: Header<'a>,
}

impl<'a> File<'a> {
    pub fn new(content: &'a [u8], head: Header<'a>) -> Self {
        Self { content, head }
    }
}

#[cfg(test)]
mod tests;
