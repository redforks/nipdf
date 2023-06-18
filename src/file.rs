//! Contains types of PDF file structures.

use anyhow::Result as AnyResult;
use itertools::Itertools;
use std::{borrow::Cow, collections::HashMap, num::NonZeroUsize, str::from_utf8};

use crate::{
    object::{Dictionary, FrameSet, Name, Object, ObjectValueError},
    parser::{parse_frame_set, parse_header, parse_indirected_object},
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
        for (id, entry) in frame_set.iter().rev().flat_map(|f| f.xref_section.iter()) {
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
    fn get_value<'b: 'a>(&self, key: &'b str) -> Option<&Object<'a>>;
}

impl<'a> DataContainer<'a> for Dictionary<'a> {
    fn get_value<'b: 'a>(&self, key: &'b str) -> Option<&Object<'a>> {
        debug_assert!(!key.starts_with('/'));
        self.get(&Name::borrowed(key.as_bytes()))
    }
}

/// Get value from first dictionary that contains `key`.
impl<'a> DataContainer<'a> for Vec<&Dictionary<'a>> {
    fn get_value<'b: 'a>(&self, key: &'b str) -> Option<&Object<'a>> {
        debug_assert!(!key.starts_with('/'));
        let key = Name::borrowed(key.as_bytes());
        for dict in self {
            if let Some(v) = dict.get(&key) {
                return Some(v);
            }
        }
        None
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
        fn parse_object(id: u32, buf: &[u8]) -> Object<'_> {
            let (_, indirect_obj) = parse_indirected_object(buf).unwrap();
            assert_eq!(id, indirect_obj.id().id());
            indirect_obj.take()
        }

        self.lru
            .get_or_insert(id, || {
                let mut o = self
                    .xref_table
                    .resolve_object_buf(id)
                    .map(|buf| parse_object(id, buf))?;
                while let Object::Reference(id) = &o {
                    let id = id.id().id();
                    o = self
                        .xref_table
                        .resolve_object_buf(id)
                        .map(|buf| parse_object(id, buf))?;
                }
                Some(o)
            })
            .as_ref()
    }

    /// Resolve value from data container `c` with key `k`, if value is reference,
    /// resolve it recursively.
    /// Return `None` if key is not found, or if value is reference
    /// but target is not found.
    pub fn resolve_value<'b: 'a, 'd: 'c, 'c, C: DataContainer<'a>>(
        &'d mut self,
        c: &'c C,
        id: &'b str,
    ) -> Option<&'c Object<'a>> {
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

    pub fn ver(&self) -> Result<Option<&[u8]>, ObjectValueError> {
        self.dict
            .get(&Name::borrowed(b"Version"))
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
            .get(&Name::borrowed(b"Size"))
            .ok_or(ObjectValueError::DictNameMissing)
            .and_then(|obj| obj.as_int())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct File {
    ver: String,
    pub total_objects: u32,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum FileError {
    #[error("catalog object is required")]
    CatalogRequired,
    #[error("missing required trailer value")]
    MissingRequiredTrailerValue,
}

impl File {
    pub fn parse(buf: &[u8]) -> AnyResult<(Self, ObjectResolver)> {
        let (_, head_ver) = parse_header(buf).unwrap();
        let (_, frame_set) = parse_frame_set(buf).unwrap();
        let xref = XRefTable::from_frame_set(buf, &frame_set);
        let mut resolver = ObjectResolver::new(xref);

        let trailers = frame_set.iter().map(|f| &f.trailer).collect_vec();
        let catalog = resolver
            .resolve_value(&trailers, "Root")
            .ok_or(FileError::CatalogRequired)?;
        let ver = catalog
            .as_dict()?
            .get(&Name::borrowed(b"Version"))
            .map(|o| -> Result<String, ObjectValueError> {
                Ok(from_utf8(o.as_name()?.as_ref()).unwrap().to_owned())
            })
            .unwrap_or(Ok(head_ver.to_owned()))?;
        let total_objects = resolver
            .resolve_value(&trailers, "Size")
            .ok_or(FileError::MissingRequiredTrailerValue)?
            .as_int()? as u32;

        Ok((Self { ver, total_objects }, resolver))
    }

    pub fn version(&self) -> &str {
        &self.ver
    }
}

#[cfg(test)]
mod tests;
