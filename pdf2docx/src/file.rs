//! Contains types of PDF file structures.

use anyhow::Result as AnyResult;
use itertools::Itertools;
use nom::Finish;
use once_cell::unsync::OnceCell;
use pdf2docx_macro::pdf_object;
use std::collections::HashMap;

use crate::{
    object::{Dictionary, FrameSet, Name, Object, ObjectValueError, RootPdfObject, SchemaDict},
    parser::{parse_frame_set, parse_header, parse_indirected_object},
};
use log::error;

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

    #[cfg(test)]
    pub fn empty() -> Self {
        Self {
            buf: &[],
            id_offset: IDOffsetMap::default(),
        }
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
                    .finish()
                    .map(|(_, o)| o.take())
                    .map_err(ObjectValueError::from)
            })
    }

    pub fn iter_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.id_offset.keys().copied()
    }

    pub fn count(&self) -> usize {
        self.id_offset.len()
    }
}

pub trait DataContainer<'a> {
    fn get_value(&self, key: &'a str) -> Option<&Object<'a>>;
}

impl<'a> DataContainer<'a> for Dictionary<'a> {
    fn get_value(&self, key: &'a str) -> Option<&Object<'a>> {
        debug_assert!(!key.starts_with('/'));
        self.get(&Name::borrowed(key.as_bytes()))
    }
}

/// Get value from first dictionary that contains `key`.
impl<'a> DataContainer<'a> for Vec<&Dictionary<'a>> {
    fn get_value(&self, key: &'a str) -> Option<&Object<'a>> {
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

pub struct ObjectResolver<'a> {
    xref_table: XRefTable<'a>,
    objects: HashMap<u32, OnceCell<Object<'a>>, BuildNoHashHasher<u32>>,
}

impl<'a> ObjectResolver<'a> {
    pub fn new(xref_table: XRefTable<'a>) -> Self {
        let mut objects =
            HashMap::with_capacity_and_hasher(xref_table.count(), BuildNoHashHasher::default());
        xref_table.iter_ids().for_each(|id| {
            objects.insert(id, OnceCell::new());
        });

        Self {
            xref_table,
            objects,
        }
    }

    #[cfg(test)]
    pub fn empty() -> Self {
        Self {
            xref_table: XRefTable::empty(),
            objects: HashMap::default(),
        }
    }

    #[cfg(test)]
    pub fn setup_object(&mut self, id: u32, v: Object<'a>) {
        self.objects.insert(id, OnceCell::with_value(v));
    }

    pub fn resolve_pdf_object<'b, T: RootPdfObject<'a, 'b>>(
        &'b self,
        id: u32,
    ) -> Result<T, ObjectValueError> {
        let obj = self.resolve(id)?.as_dict()?;
        T::new(id, obj, self)
    }

    pub fn opt_resolve_pdf_object<'b, T: RootPdfObject<'a, 'b>>(
        &'b self,
        id: u32,
    ) -> Result<Option<T>, ObjectValueError> {
        Self::to_opt(self.resolve_pdf_object(id))
    }

    /// Resolve object with id `id`, if object is reference, resolve it recursively.
    /// Return `None` if object is not found.
    pub fn opt_resolve(&self, id: u32) -> Result<Option<&Object<'a>>, ObjectValueError> {
        Self::to_opt(self.resolve(id))
    }

    /// Resolve object with id `id`, if object is reference, resolve it recursively.
    pub fn resolve(&self, id: u32) -> Result<&Object<'a>, ObjectValueError> {
        self.objects
            .get(&id)
            .ok_or(ObjectValueError::ObjectIDNotFound)?
            .get_or_try_init(|| {
                let mut o = self.xref_table.parse_object(id)?;
                loop {
                    match o {
                        Object::Reference(id) => {
                            let id = id.id().id();
                            o = self.xref_table.parse_object(id)?;
                        }
                        _ => break,
                    }
                }
                Ok(o)
            })
    }

    /// Resolve value from data container `c` with key `k`, if value is reference,
    /// resolve it recursively. Return `None` if object is not found.
    pub fn opt_resolve_container_value<'b: 'a, 'd: 'c, 'c, C: DataContainer<'a>>(
        &'d self,
        c: &'c C,
        id: &'b str,
    ) -> Result<Option<&'c Object<'a>>, ObjectValueError> {
        Self::to_opt(self.resolve_container_value(c, id))
    }

    /// Resolve value from data container `c` with key `k`, if value is reference,
    /// resolve it recursively.
    pub fn resolve_container_value<'b: 'a, 'd: 'c, 'c, C: DataContainer<'a>>(
        &'d self,
        c: &'c C,
        id: &'b str,
    ) -> Result<&'c Object<'a>, ObjectValueError> {
        self._resolve_container_value(c, id).map(|(_, o)| o)
    }

    pub fn opt_resolve_container_pdf_object<
        'b: 'a,
        'd: 'c,
        'c,
        C: DataContainer<'a>,
        T: RootPdfObject<'a, 'c>,
    >(
        &'d self,
        c: &'c C,
        id: &'b str,
    ) -> Result<Option<T>, ObjectValueError> {
        Self::to_opt(self.resolve_container_pdf_object(c, id))
    }

    pub fn resolve_container_pdf_object<
        'b: 'a,
        'd: 'c,
        'c,
        C: DataContainer<'a>,
        T: RootPdfObject<'a, 'c>,
    >(
        &'d self,
        c: &'c C,
        id: &'b str,
    ) -> Result<T, ObjectValueError> {
        let (id, obj) = self._resolve_container_value(c, id)?;
        let obj = obj.as_dict()?;
        T::new(id.expect("Should be root pdf object"), obj, self)
    }

    fn _resolve_container_value<'b: 'a, 'd: 'c, 'c, C: DataContainer<'a>>(
        &'d self,
        c: &'c C,
        id: &'b str,
    ) -> Result<(Option<u32>, &'c Object<'a>), ObjectValueError> {
        let obj = c.get_value(id).ok_or(ObjectValueError::ObjectIDNotFound)?;

        if let Object::Reference(id) = obj {
            self.resolve(id.id().id()).map(|o| (Some(id.id().id()), o))
        } else {
            Ok((None, obj))
        }
    }

    /// Resolve root pdf_objects from data container `c` with key `k`, if value is reference,
    /// resolve it recursively. Return empty vector if object is not found.
    /// The raw value should be an array of references.
    pub fn resolve_container_pdf_object_array<
        'b: 'a,
        'd: 'c,
        'c,
        C: DataContainer<'a>,
        T: RootPdfObject<'a, 'c>,
    >(
        &'d self,
        c: &'c C,
        id: &'b str,
    ) -> Result<Vec<T>, ObjectValueError> {
        let arr = c.get_value(id);
        arr.map_or_else(
            || Ok(vec![]),
            |arr| {
                let arr = arr.as_arr()?;
                let mut res = Vec::with_capacity(arr.len());
                for obj in arr {
                    let id = obj.as_ref()?;
                    let dict = self.resolve(id.id().id()).map(|o| o)?;
                    res.push(T::new(id.id().id(), dict.as_dict()?, self)?);
                }
                Ok(res)
            },
        )
    }

    fn to_opt<T>(o: Result<T, ObjectValueError>) -> Result<Option<T>, ObjectValueError> {
        o.map(Some).or_else(|e| {
            if let ObjectValueError::ObjectIDNotFound = e {
                Ok(None)
            } else {
                Err(e)
            }
        })
    }
}

#[pdf_object("Catalog")]
#[root_object]
trait CatalogDictTrait {
    #[typ("Name")]
    fn version(&self) -> Option<&str>;
    #[nested_root]
    fn pages(&self) -> PageDict<'a, 'b>;
}

#[derive(Debug)]
pub struct Catalog {
    id: u32,
    pages: Vec<Page>,
    ver: Option<String>,
}

impl Catalog {
    fn parse(id: u32, resolver: &mut ObjectResolver) -> Result<Self, ObjectValueError> {
        let catalog_dict: CatalogDict = resolver.resolve_pdf_object(id)?;
        let dict = resolver.resolve(id)?.as_dict()?;
        let dict = SchemaDict::new(dict, resolver, "Catalog")?;

        let root_page_id = dict.required_ref("Pages")?;
        let ver = catalog_dict.version().map(|s| s.to_owned());
        let pages = Page::parse(resolver.resolve_pdf_object(root_page_id)?, resolver)?;
        Ok(Self { id, pages, ver })
    }

    pub fn pages(&self) -> &[Page] {
        self.pages.as_slice()
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

#[derive(Debug)]
pub struct File {
    ver: String,
    total_objects: u32,
    catalog: Catalog,
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
        let root_id = trailers
            .iter()
            .find_map(|t| t.get(&Name::borrowed(b"Root")))
            .unwrap();
        let root_id = root_id.as_ref().unwrap().id().id();
        let catalog = resolver
            .resolve_container_value(&trailers, "Root")
            .map_err(|_| FileError::CatalogRequired)?;
        let ver = catalog
            .as_dict()?
            .get(&Name::borrowed(b"Version"))
            .map(|o| -> Result<String, ObjectValueError> { Ok(o.as_name().unwrap().to_owned()) })
            .unwrap_or(Ok(head_ver.to_owned()))?;
        let total_objects = resolver
            .resolve_container_value(&trailers, "Size")
            .map_err(|_| FileError::MissingRequiredTrailerValue)?
            .as_int()? as u32;
        let catalog = Catalog::parse(root_id, &mut resolver)?;

        Ok((
            Self {
                ver,
                total_objects,
                catalog,
            },
            resolver,
        ))
    }

    pub fn version(&self) -> &str {
        &self.ver
    }

    pub fn total_objects(&self) -> u32 {
        self.total_objects
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }
}

#[cfg(test)]
mod tests;
