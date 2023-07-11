//! Contains types of PDF file structures.

use anyhow::Result as AnyResult;
use itertools::Itertools;
use nom::bytes::complete::take_until;
use std::{collections::HashMap, num::NonZeroUsize, str::from_utf8};

use crate::{
    object::{Dictionary, FrameSet, Name, Object, ObjectValueError, Stream},
    parser::{
        parse_frame_set, parse_header, parse_indirected_object, parse_stream_content, ParseError,
    },
};
use log::error;
use lru::LruCache;
use nohash_hasher::BuildNoHashHasher;

mod page;
pub use page::*;

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

    pub fn parse_object(&self, id: u32) -> Result<Object<'a>, ObjectValueError> {
        self.resolve_object_buf(id)
            .ok_or(ObjectValueError::ObjectIDNotFound)
            .and_then(|buf| {
                parse_indirected_object(buf)
                    .map(|(_, o)| o.take())
                    .map_err(|e| ObjectValueError::ParseError(e.to_string()))
            })
    }
}

pub struct ObjectResolver<'a> {
    xref_table: XRefTable<'a>,
    lru: LruCache<u32, Object<'a>>,
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

    #[cfg(test)]
    pub fn empty() -> Self {
        Self {
            xref_table: XRefTable::new(&[], IDOffsetMap::default()),
            lru: LruCache::new(NonZeroUsize::new(5000).unwrap()),
        }
    }

    #[cfg(test)]
    pub fn setup_object(&mut self, id: u32, v: Object<'a>) {
        self.lru.push(id, v);
    }

    /// Resolve object with id `id`, if object is reference, resolve it recursively.
    pub fn resolve(&mut self, id: u32) -> Result<&Object<'a>, ObjectValueError> {
        self.lru.try_get_or_insert(id, || {
            let mut o = self.xref_table.parse_object(id)?;
            loop {
                match o {
                    Object::Reference(id) => {
                        let id = id.id().id();
                        o = self.xref_table.parse_object(id)?;
                    }
                    Object::LaterResolveStream(d) => {
                        let l = d.get(&Name::borrowed(b"Length")).unwrap();
                        let l = l.as_reference().unwrap();
                        let l = self.xref_table.parse_object(l.id().id()).unwrap();
                        let l = l.as_int().unwrap();
                        let buf = self.xref_table.resolve_object_buf(id).unwrap();
                        let (buf, _) =
                            take_until::<&[u8], &[u8], ParseError>(b"stream".as_slice())(buf)
                                .unwrap();
                        let (_, content) = parse_stream_content(buf, l as u32).unwrap();
                        o = Object::Stream(Stream(d, content));
                    }
                    _ => break,
                }
            }
            Ok(o)
        })
    }

    /// Resolve value from data container `c` with key `k`, if value is reference,
    /// resolve it recursively.
    /// Return `None` if key is not found, or if value is reference
    /// but target is not found.
    pub fn resolve_value<'b: 'a, 'd: 'c, 'c, C: DataContainer<'a>>(
        &'d mut self,
        c: &'c C,
        id: &'b str,
    ) -> Result<&'c Object<'a>, ObjectValueError> {
        let obj = c.get_value(id).ok_or(ObjectValueError::ObjectIDNotFound)?;

        if let Object::Reference(id) = obj {
            self.resolve(id.id().id())
        } else {
            Ok(obj)
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Catalog<'a> {
    dict: Dictionary<'a>,
}

impl<'a> Catalog<'a> {
    pub fn new(dict: Dictionary<'a>) -> Self {
        assert_eq!("Catalog", dict.get_name("Type").unwrap().unwrap());
        Self { dict }
    }

    pub fn ver(&self) -> Option<&str> {
        self.dict.get_name("Version").unwrap()
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
            .map_err(|_| FileError::CatalogRequired)?;
        let ver = catalog
            .as_dict()?
            .get(&Name::borrowed(b"Version"))
            .map(|o| -> Result<String, ObjectValueError> {
                Ok(from_utf8(o.as_name()?).unwrap().to_owned())
            })
            .unwrap_or(Ok(head_ver.to_owned()))?;
        let total_objects = resolver
            .resolve_value(&trailers, "Size")
            .map_err(|_| FileError::MissingRequiredTrailerValue)?
            .as_int()? as u32;

        Ok((Self { ver, total_objects }, resolver))
    }

    pub fn version(&self) -> &str {
        &self.ver
    }
}

#[cfg(test)]
mod tests;
