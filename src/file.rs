//! Contains types of PDF file structures.

use std::{borrow::Cow, collections::HashMap, num::NonZeroUsize};

use crate::{
    object::{Dictionary, FrameSet, Name, Object, ObjectValueError},
    parser::parse_object,
};
use lru::LruCache;
use nohash_hasher::BuildNoHashHasher;

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

pub trait DataContainer<'a> {
    fn get_value(&'a self, key: &'a str) -> Option<&'a Object>;
}

impl<'a> DataContainer<'a> for Dictionary<'a> {
    fn get_value(&'a self, key: &'a str) -> Option<&'a Object> {
        debug_assert!(key.starts_with('/'));
        self.get(&Name::new(key.as_bytes()))
    }
}

impl<'a> ObjectResolver<'a> {
    pub fn new(xref_table: XRefTable<'a>) -> Self {
        Self {
            xref_table,
            lru: LruCache::new(NonZeroUsize::new(5000).unwrap()),
        }
    }

    /// Resolve object with id `id`, if object is reference, resolve it recursively.
    pub fn resolve(&mut self, id: u32) -> Option<&Object<'a>> {
        self.lru
            .get_or_insert(id, || {
                let mut o = self
                    .xref_table
                    .resolve_object_buf(id)
                    .map(|buf| parse_object(buf).unwrap().1)?;
                while let Object::Reference(id) = &o {
                    let id = id.id().id();
                    o = self
                        .xref_table
                        .resolve_object_buf(id)
                        .map(|buf| parse_object(buf).unwrap().1)?;
                }
                Some(o)
            })
            .as_ref()
    }

    /// Resolve value from data container `c` with key `k`, if value is reference,
    /// resolve it recursively.
    /// Return `None` if key is not found, or if value is reference
    /// but target is not found.
    pub fn resolve_value<K, C: DataContainer<'a>>(
        &mut self,
        c: &'a C,
        id: &'a str,
    ) -> Option<&Object<'a>> {
        let obj = c.get_value(id)?;

        if let Object::Reference(id) = obj {
            self.resolve(id.id().id())
        } else {
            Some(obj)
        }
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
}

impl<'a> File<'a> {
    pub fn new(content: &'a [u8]) -> Self {
        Self { content }
    }
}

#[cfg(test)]
mod tests;
