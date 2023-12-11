//! Contains types of PDF file structures.

use crate::{
    file::encrypt::{calc_encrypt_key, Algorithm, Decryptor, Rc4Decryptor, StandardHandlerRevion},
    object::{
        Array, Dictionary, Entry, FrameSet, HexString, LiteralString, Object, ObjectId,
        ObjectValueError, PdfObject, Resolver, Stream, TrailerDict,
    },
    parser::{
        parse_frame_set, parse_header, parse_indirect_object, parse_indirect_stream, parse_object,
        ParseResult,
    },
};
use ahash::{HashMap, HashMapExt};
use anyhow::Result as AnyResult;
use either::Either;
use log::error;
use nipdf_macro::pdf_object;
use nom::Finish;
use once_cell::unsync::OnceCell;
use prescript::{sname, Name};
use std::{iter::repeat_with, num::NonZeroU32, rc::Rc};

pub mod page;
pub use page::*;

pub(crate) mod encrypt;

pub use encrypt::EncryptDict;

#[derive(Debug, Copy, Clone)]
pub enum ObjectPos {
    Offset(u32),
    InStream(NonZeroU32, u16),
}

impl<'a> From<&'a Entry> for ObjectPos {
    fn from(e: &'a Entry) -> Self {
        match e {
            Entry::InFile(pos) => ObjectPos::Offset(pos.offset()),
            Entry::InStream(id, idx) => ObjectPos::InStream(*id, *idx),
        }
    }
}

type IDOffsetMap = HashMap<u32, ObjectPos>;

/// Object stream stores multiple objects in a stream. See section 7.5.7
#[derive(Debug)]
struct ObjectStream {
    /// Data contains all objects in this stream, without index part.
    buf: Vec<u8>,
    /// offsets of objects in `buf`
    offsets: Vec<u16>,
}

fn parse_object_stream(n: usize, buf: &[u8]) -> ParseResult<ObjectStream> {
    use nom::{
        character::complete::{space0, space1, u16, u32},
        multi::count,
        sequence::{separated_pair, terminated},
    };

    let (buf, nums) = count(terminated(separated_pair(u32, space1, u16), space0), n)(buf)?;
    let offsets = nums.into_iter().map(|(_, n)| n).collect();
    Ok((
        buf,
        ObjectStream {
            buf: buf.to_owned(),
            offsets,
        },
    ))
}

impl ObjectStream {
    pub fn new(stream: Stream, file: &[u8]) -> Result<Self, ObjectValueError> {
        let d = stream.as_dict();
        assert_eq!(sname("ObjStm"), d[&sname("Type")].name()?);
        let n = d.get(&sname("N")).map_or(Ok(0), |v| v.int())? as usize;
        assert!(
            !d.contains_key(&sname("Extends")),
            "Extends is not supported"
        );
        let buf = stream.decode_without_resolve_length(file)?;
        parse_object_stream(n, buf.as_ref())
            .map_err(|e| ObjectValueError::ParseError(e.to_string()))
            .map(|(_, r)| r)
    }

    pub fn get_buf(&self, idx: usize) -> &[u8] {
        let start = self.offsets[idx] as usize;
        let end = if idx == self.offsets.len() - 1 {
            self.buf.len()
        } else {
            self.offsets[idx + 1] as usize
        };
        &self.buf[start..end]
    }
}

#[derive(Debug)]
pub struct XRefTable {
    id_offset: IDOffsetMap,
    // object id -> offset
    object_streams: HashMap<NonZeroU32, OnceCell<ObjectStream>>, // stream id -> ObjectStream
}

impl XRefTable {
    pub fn new(id_offset: IDOffsetMap) -> Self {
        let object_stream = id_offset
            .values()
            .filter_map(|e| {
                if let ObjectPos::InStream(id, _) = e {
                    Some(*id)
                } else {
                    None
                }
            })
            .zip(repeat_with(OnceCell::new))
            .collect();

        Self {
            id_offset,
            object_streams: object_stream,
        }
    }

    #[cfg(test)]
    pub fn empty() -> Self {
        Self {
            id_offset: IDOffsetMap::default(),
            object_streams: HashMap::new(),
        }
    }

    /// Scan IDOffsetMap by scan indirect object declaration,
    /// helps to create pdf file objects for testing.
    pub fn from_buf(buf: &[u8]) -> Self {
        use crate::parser::{whitespace_or_comment, ws_prefixed};
        use nom::{combinator::all_consuming, multi::many1};

        let (input, objects) = many1(ws_prefixed(parse_indirect_object))(buf).unwrap();
        all_consuming(whitespace_or_comment)(input).unwrap();
        let mut id_offset = IDOffsetMap::new();
        for o in objects {
            let search_key = format!("{} {} obj", o.id().id(), o.id().generation());
            let pos = buf
                .windows(search_key.len())
                .position(|w| w == search_key.as_bytes())
                .unwrap() as u32;
            id_offset.insert(o.id().id().into(), ObjectPos::Offset(pos));
        }

        Self::new(id_offset)
    }

    fn scan(frame_set: &FrameSet) -> IDOffsetMap {
        let mut r = IDOffsetMap::with_capacity(5000);
        for (id, entry) in frame_set.iter().rev().flat_map(|f| f.xref_section.iter()) {
            if entry.is_used() {
                r.insert(*id, entry.into());
            } else if *id != 0 {
                r.remove(id);
            }
        }
        r
    }

    pub fn from_frame_set(frame_set: &FrameSet) -> Self {
        Self::new(Self::scan(frame_set))
    }

    /// Return `buf` start from where `id` is
    fn resolve_object_buf<'a, 'b>(
        &'b self,
        buf: &'a [u8],
        id: NonZeroU32,
    ) -> Option<Either<&'a [u8], &'b [u8]>> {
        self.id_offset.get(&id.into()).map(|entry| match entry {
            ObjectPos::Offset(offset) => Either::Left(&buf[*offset as usize..]),
            ObjectPos::InStream(id, idx) => {
                let object_stream = self.object_streams[id]
                    .get_or_try_init(|| {
                        let buf = self.resolve_object_buf(buf, *id).unwrap();
                        let (_, stream) = parse_indirect_stream(&buf).unwrap();
                        ObjectStream::new(stream, &buf)
                    })
                    .unwrap();
                Either::Right(object_stream.get_buf(*idx as usize))
            }
        })
    }

    pub fn parse_object<'a: 'c, 'b: 'c, 'c>(
        &'b self,
        buf: &'a [u8],
        id: NonZeroU32,
        encrypt_info: Option<&EncryptInfo>,
    ) -> Result<Object, ObjectValueError> {
        self.resolve_object_buf(buf, id)
            .ok_or(ObjectValueError::ObjectIDNotFound(id))
            .and_then(|buf| {
                buf.either(
                    |buf| {
                        parse_indirect_object(buf)
                            .finish()
                            .map(|(_, o)| {
                                let id = o.id();
                                let o = o.take();
                                if let Some(encrypt_info) = encrypt_info {
                                    decrypt_string(encrypt_info, id, o)
                                } else {
                                    o
                                }
                            })
                            .map_err(ObjectValueError::from)
                    },
                    |buf| {
                        parse_object(buf)
                            .finish()
                            .map(|(_, o)| o)
                            .map_err(ObjectValueError::from)
                    },
                )
            })
    }

    pub fn iter_ids(&self) -> impl Iterator<Item = NonZeroU32> + '_ {
        self.id_offset.keys().map(|v| NonZeroU32::new(*v).unwrap())
    }

    pub fn count(&self) -> usize {
        self.id_offset.len()
    }
}

/// Decrypt HexString/LiteralString nested in object.
fn decrypt_string(encrypt_info: &EncryptInfo, id: ObjectId, mut o: Object) -> Object {
    struct Decryptor<T>(T);

    impl<T: crate::file::Decryptor> Decryptor<T> {
        pub fn new(decryptor: T) -> Self {
            Self(decryptor)
        }

        fn hex_string(&self, s: &mut HexString) {
            self.0.decrypt(&mut s.0);
        }

        fn literal_string(&self, s: &mut LiteralString) {
            self.0.decrypt(&mut s.0);
        }

        fn dict(&self, dict: &mut Dictionary) {
            dict.update(|d| {
                for (_, v) in d.iter_mut() {
                    self.decrypt(v);
                }
            })
        }

        fn arr(&self, arr: &mut Array) {
            Object::update_array_items(arr, |o| self.decrypt(o));
        }

        fn stream(&self, stream: &mut Rc<Stream>) {
            let stream = Rc::make_mut(stream);
            self.dict(&mut stream.0);
        }

        fn decrypt(&self, o: &mut Object) {
            match o {
                Object::HexString(s) => self.hex_string(s),
                Object::LiteralString(s) => self.literal_string(s),
                Object::Dictionary(d) => self.dict(d),
                Object::Array(arr) => self.arr(arr),
                Object::Stream(s) => self.stream(s),
                _ => {}
            }
        }
    }

    match encrypt_info.algorithm {
        Algorithm::Key40 | Algorithm::Key40AndMore => {
            Decryptor::new(Rc4Decryptor::new(&encrypt_info.encript_key, id)).decrypt(&mut o);
        }
        _ => todo!(),
    }
    o
}

pub trait DataContainer {
    fn get_value(&self, key: &Name) -> Option<&Object>;
}

impl DataContainer for Dictionary {
    fn get_value(&self, key: &Name) -> Option<&Object> {
        self.get(key)
    }
}

/// Get value from first dictionary that contains `key`.
impl DataContainer for Vec<&Dictionary> {
    fn get_value(&self, key: &Name) -> Option<&Object> {
        self.iter().find_map(|d| d.get(key))
    }
}

#[derive(Clone)]
pub struct EncryptInfo {
    pub encript_key: Box<[u8]>,
    pub algorithm: Algorithm,
}

impl EncryptInfo {
    pub fn new(encript_key: Box<[u8]>, algorithm: Algorithm) -> Self {
        Self {
            encript_key,
            algorithm,
        }
    }
}

pub struct ObjectResolver<'a> {
    buf: &'a [u8],
    xref_table: &'a XRefTable,
    objects: HashMap<NonZeroU32, OnceCell<Object>>,
    encript_info: Option<EncryptInfo>,
}

impl<'a> ObjectResolver<'a> {
    pub fn new(
        buf: &'a [u8],
        xref_table: &'a XRefTable,
        encript_info: Option<EncryptInfo>,
    ) -> Self {
        let mut objects = HashMap::with_capacity(xref_table.count());
        xref_table.iter_ids().for_each(|id| {
            objects.insert(id, OnceCell::new());
        });

        Self {
            buf,
            xref_table,
            objects,
            encript_info,
        }
    }

    pub fn encript_info(&self) -> Option<&EncryptInfo> {
        self.encript_info.as_ref()
    }

    /// Return total objects count.
    #[allow(dead_code)]
    pub fn n(&self) -> usize {
        self.objects.len()
    }

    #[cfg(test)]
    pub fn empty(xref_table: &'a XRefTable) -> Self {
        Self {
            buf: b"",
            xref_table,
            objects: HashMap::default(),
            encript_info: None,
        }
    }

    #[cfg(test)]
    pub fn setup_object(&mut self, id: u32, v: Object) {
        self.objects
            .insert(NonZeroU32::new(id).unwrap(), OnceCell::with_value(v));
    }

    /// Resolve pdf object from object, if object is dict, use it as pdf object,
    /// if object is reference, resolve it
    pub fn resolve_pdf_object2<'b, T: PdfObject<'b, Self>>(
        &'b self,
        o: &'b Object,
    ) -> Result<T, ObjectValueError> {
        match o {
            Object::Reference(ref_id) => self.resolve_pdf_object(ref_id.id().id()),
            _ => T::new(None, o.as_dict()?, self),
        }
    }

    pub fn resolve_pdf_object<'b, T: PdfObject<'b, Self>>(
        &'b self,
        id: NonZeroU32,
    ) -> Result<T, ObjectValueError> {
        let obj = self.resolve(id)?.as_dict()?;
        T::new(Some(id), obj, self)
    }

    /// Resolve object with id `id`.
    pub fn resolve(&self, id: NonZeroU32) -> Result<&Object, ObjectValueError> {
        self.objects
            .get(&id)
            .ok_or(ObjectValueError::ObjectIDNotFound(id))?
            .get_or_try_init(|| {
                self.xref_table
                    .parse_object(self.buf, id, self.encript_info())
            })
    }

    /// Return file data start from stream id indirect object till the file end
    /// Panic if id not found or not stream
    pub fn stream_data(&self, id: NonZeroU32) -> &'a [u8] {
        self.xref_table
            .resolve_object_buf(self.buf, id)
            .unwrap()
            .unwrap_left()
    }

    /// Resolve value from data container `c` with key `k`, if value is reference,
    /// resolve it recursively. Return `None` if object is not found.
    pub fn opt_resolve_container_value<'b: 'c, 'c, C: DataContainer>(
        &'b self,
        c: &'c C,
        id: &Name,
    ) -> Result<Option<&'c Object>, ObjectValueError> {
        Self::not_found_error_to_opt(self._resolve_container_value(c, id).map(|(_, o)| o))
    }

    /// Resolve value from data container `c` with key `k`, if value is reference,
    /// resolve it recursively.
    pub fn resolve_container_value<'b: 'c, 'c, C: DataContainer>(
        &'b self,
        c: &'c C,
        id: &Name,
    ) -> Result<&'c Object, ObjectValueError> {
        self.resolve_required_value(c, id).map(|(_, o)| o)
    }

    /// Like _resolve_container_value(), but error logs if value not exist
    fn resolve_required_value<'b: 'c, 'c, C: DataContainer>(
        &'b self,
        c: &'c C,
        id: &Name,
    ) -> Result<(Option<NonZeroU32>, &'c Object), ObjectValueError> {
        self._resolve_container_value(c, id).map_err(|e| {
            error!("{}: {}", e, id);
            e
        })
    }

    fn _resolve_container_value<'b: 'c, 'c, C: DataContainer>(
        &'b self,
        c: &'c C,
        id: &Name,
    ) -> Result<(Option<NonZeroU32>, &'c Object), ObjectValueError> {
        let obj = c.get_value(id).ok_or(ObjectValueError::DictKeyNotFound)?;

        if let Object::Reference(id) = obj {
            self.resolve(id.id().id()).map(|o| (Some(id.id().id()), o))
        } else {
            Ok((None, obj))
        }
    }

    /// Resolve pdf_object by id, if its end value is dictionary, return with one element vec.
    /// If its end value is array, return all elements in array.
    pub fn resolve_one_or_more_pdf_object<'b, T: PdfObject<'b, Self>>(
        &'b self,
        id: NonZeroU32,
    ) -> Result<Vec<T>, ObjectValueError> {
        let obj = self.resolve(id)?;
        match obj {
            Object::Dictionary(d) => Ok(vec![T::new(Some(id), d, self)?]),
            Object::Stream(s) => Ok(vec![T::new(Some(id), s.as_dict(), self)?]),
            Object::Array(arr) => {
                let mut res = Vec::with_capacity(arr.len());
                for obj in arr.iter() {
                    let dict = self.resolve_reference(obj)?;
                    res.push(T::new(
                        obj.reference().ok().map(|id| id.id().id()),
                        dict.as_dict()?,
                        self,
                    )?);
                }
                Ok(res)
            }
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }

    fn not_found_error_to_opt<T>(
        o: Result<T, ObjectValueError>,
    ) -> Result<Option<T>, ObjectValueError> {
        o.map(Some).or_else(|e| match e {
            ObjectValueError::ObjectIDNotFound(_) | ObjectValueError::DictKeyNotFound => Ok(None),
            _ => Err(e),
        })
    }
}

impl<'a> Resolver for ObjectResolver<'a> {
    fn do_resolve_container_value<'b: 'c, 'c, C: DataContainer>(
        &'b self,
        c: &'c C,
        id: &Name,
    ) -> Result<(Option<NonZeroU32>, &'c Object), ObjectValueError> {
        self._resolve_container_value(c, id)
    }

    fn resolve_reference<'b>(&'b self, v: &'b Object) -> Result<&'b Object, ObjectValueError> {
        if let Object::Reference(id) = v {
            self.resolve(id.id().id())
        } else {
            Ok(v)
        }
    }
}

#[pdf_object("Catalog")]
trait CatalogDictTrait {
    fn version(&self) -> Option<Name>;
    #[nested]
    fn pages(&self) -> PageDict<'a, 'b>;
}

#[derive(Debug)]
pub struct Catalog<'a, 'b> {
    d: CatalogDict<'a, 'b>,
}

impl<'a, 'b: 'a> Catalog<'a, 'b> {
    fn parse(id: NonZeroU32, resolver: &'b ObjectResolver<'a>) -> Result<Self, ObjectValueError> {
        Ok(Self {
            d: resolver.resolve_pdf_object(id)?,
        })
    }

    pub fn pages(&self) -> Result<Vec<Page<'a, 'b>>, ObjectValueError> {
        Page::parse(self.d.pages().unwrap())
    }

    pub fn ver(&self) -> Option<Name> {
        self.d.version().unwrap()
    }
}

pub struct File {
    root_id: NonZeroU32,
    head_ver: String,
    data: Vec<u8>,
    xref: XRefTable,
    encrypt_info: Option<EncryptInfo>,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum FileError {
    #[error("catalog object is required")]
    CatalogRequired,
    #[error("missing required trailer value")]
    MissingRequiredTrailerValue,
}

/// Open possible encrypt file, return None if not encrypted.
fn open_encrypt(
    buf: &[u8],
    xref: &XRefTable,
    trailer: Option<&Dictionary>,
    _owner_password: &str,
    user_password: &str,
) -> AnyResult<Option<EncryptInfo>> {
    let Some(trailer) = trailer else {
        return Ok(None);
    };

    let resolver = ObjectResolver::new(buf, xref, None);
    let trailer = TrailerDict::new(None, trailer, &resolver)?;
    let encrypt = trailer.encrypt()?;
    let Some(encrypt) = encrypt else {
        return Ok(None);
    };

    assert_eq!(
        sname("Standard"),
        encrypt.filter()?,
        "unsupported security handler"
    );
    assert!(
        encrypt.sub_filter()?.is_none(),
        "unsupported security handler (SubFilter)"
    );
    assert!(
        StandardHandlerRevion::V3 == encrypt.revison()?
            || StandardHandlerRevion::V2 == encrypt.revison()?
    );

    let owner_hash = encrypt.owner_password_hash()?;
    let user_hash = encrypt.user_password_hash()?;
    let mut owner_hash_arr = [0u8; 32];
    let mut user_hash_arr = [0u8; 32];
    owner_hash_arr.copy_from_slice(&owner_hash[..32]);
    user_hash_arr.copy_from_slice(&user_hash[..32]);

    if encrypt::authorize_user(
        encrypt.revison()?,
        encrypt.key_length()? as usize,
        user_password.as_bytes(),
        &owner_hash_arr,
        &user_hash_arr,
        encrypt.permission_flags()?,
        &trailer.id()?.unwrap().0,
    ) {
        let key = calc_encrypt_key(
            encrypt.revison()?,
            encrypt.key_length()? as usize,
            user_password.as_bytes(),
            &owner_hash_arr,
            encrypt.permission_flags()?,
            &trailer.id()?.unwrap().0,
        );
        Ok(Some(EncryptInfo::new(key, encrypt.algorithm()?)))
    } else {
        Ok(None)
    }
}

impl File {
    pub fn parse(buf: Vec<u8>, owner_password: &str, user_password: &str) -> AnyResult<Self> {
        let (_, head_ver) = parse_header(&buf).unwrap();
        let (_, frame_set) = parse_frame_set(&buf).unwrap();
        let xref = XRefTable::from_frame_set(&frame_set);

        let trailers: Vec<_> = frame_set.into_iter().map(|f| f.trailer).collect();
        let encrypt_key = open_encrypt(
            &buf,
            &xref,
            trailers.iter().find(|d| d.contains_key(&sname("Encrypt"))),
            owner_password,
            user_password,
        )?;

        let root_id = trailers.iter().find_map(|t| t.get(&sname("Root"))).unwrap();
        let root_id = root_id.reference().unwrap().id().id();

        Ok(Self {
            head_ver: head_ver.to_owned(),
            root_id,
            data: buf,
            xref,
            encrypt_info: encrypt_key,
        })
    }

    pub fn resolver(&self) -> AnyResult<ObjectResolver<'_>> {
        Ok(ObjectResolver::new(
            &self.data,
            &self.xref,
            self.encrypt_info.clone(),
        ))
    }

    pub fn version<'a>(
        &'a self,
        resolver: &'a ObjectResolver<'a>,
    ) -> Result<String, ObjectValueError> {
        let catalog = self.catalog(resolver)?;
        Ok(catalog
            .ver()
            .map(|s| s.into_string())
            .unwrap_or_else(|| self.head_ver.clone()))
    }

    pub fn catalog<'a, 'b: 'a>(
        &self,
        resolver: &'b ObjectResolver<'a>,
    ) -> Result<Catalog<'a, 'b>, ObjectValueError> {
        Catalog::parse(self.root_id, resolver)
    }
}

/// Decode stream for testing. `file_path` relate to current crate directory.
/// `f_assert` called with `Dictionary` of stream to do some test on it.
#[cfg(test)]
pub(crate) fn decode_stream<
    E: std::error::Error + Sync + Send + 'static,
    T: TryInto<NonZeroU32, Error = E>,
>(
    file_path: impl AsRef<std::path::Path>,
    id: T,
    f_assert: impl for<'a> FnOnce(&'a Dictionary, &'a ObjectResolver<'a>) -> AnyResult<()>,
) -> AnyResult<Vec<u8>> {
    let f = open_test_file(file_path);
    let resolver = f.resolver()?;
    let stream = resolver.resolve(id.try_into()?)?.stream()?;
    f_assert(stream.as_dict(), &resolver)?;
    Ok(stream.decode(&resolver)?.into_owned())
}

/// Open file for testing. `file_path` relate to current crate directory.
#[cfg(test)]
pub(crate) fn open_test_file(file_path: impl AsRef<std::path::Path>) -> File {
    let file_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file_path);
    let data = std::fs::read(file_path).unwrap();
    File::parse(data, "", "").unwrap()
}

#[cfg(test)]
mod tests;
