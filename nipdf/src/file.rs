//! Contains types of PDF file structures.

use crate::{
    file::encrypt::{calc_encrypt_key, decrypt_with_key, Algorithm, StandardHandlerRevion},
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
use prescript::Name;
use prescript_macro::name;
use std::{iter::repeat_with, num::NonZeroU32, rc::Rc};

mod page;
pub use page::*;

pub(crate) mod encrypt;
use self::encrypt::decrypt_key;
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
        assert_eq!(name!("ObjStm"), d[&name!("Type")].name()?);
        let n = d.get(&name!("N")).map_or(Ok(0), |v| v.int())? as usize;
        assert!(
            !d.contains_key(&name!("Extends")),
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
    #[cfg(test)]
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
        decrypt_key: Option<&[u8]>,
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
                                if let Some(key) = decrypt_key {
                                    decrypt_string(key, id, o)
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
fn decrypt_string(key: &[u8], id: ObjectId, mut o: Object) -> Object {
    let key = decrypt_key(key, id);

    struct Decryptor(Box<[u8]>);

    impl Decryptor {
        fn hex_string(&self, s: &mut HexString) {
            s.update(|s| decrypt_with_key(&self.0, s));
        }

        fn literal_string(&self, s: &mut LiteralString) {
            s.update(|s| decrypt_with_key(&self.0, s));
        }

        fn dict(&self, dict: &mut Dictionary) {
            for (_, v) in dict.iter_mut() {
                self.decrypt(v);
            }
        }

        fn arr(&self, arr: &mut Rc<Array>) {
            match Rc::get_mut(arr) {
                Some(r) => {
                    for o in r.iter_mut() {
                        self.decrypt(o);
                    }
                }
                None => {
                    let mut r = Vec::with_capacity(arr.len());
                    for o in arr.iter() {
                        let mut o = o.clone();
                        self.decrypt(&mut o);
                        r.push(o);
                    }
                    *arr = Rc::new(r);
                }
            }
        }

        fn stream(&self, stream: &mut Rc<Stream>) {
            match Rc::get_mut(stream) {
                Some(s) => {
                    self.dict(&mut s.0);
                }
                None => {
                    let mut d = stream.as_dict().clone();
                    self.dict(&mut d);
                    *stream = Rc::new(Stream::new(d, stream.1.clone(), stream.2.clone()));
                }
            }
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

    Decryptor(key).decrypt(&mut o);
    o
}

pub trait DataContainer {
    fn get_value(&self, key: Name) -> Option<&Object>;
}

impl DataContainer for Dictionary {
    fn get_value(&self, key: Name) -> Option<&Object> {
        self.get(&key)
    }
}

/// Get value from first dictionary that contains `key`.
impl DataContainer for Vec<&Dictionary> {
    fn get_value(&self, key: Name) -> Option<&Object> {
        self.iter().find_map(|d| d.get(&key))
    }
}

pub struct ObjectResolver<'a> {
    buf: &'a [u8],
    xref_table: &'a XRefTable,
    objects: HashMap<NonZeroU32, OnceCell<Object>>,
    encript_key: Option<Box<[u8]>>,
}

impl<'a> ObjectResolver<'a> {
    pub fn new(buf: &'a [u8], xref_table: &'a XRefTable, encript_key: Option<Box<[u8]>>) -> Self {
        let mut objects = HashMap::with_capacity(xref_table.count());
        xref_table.iter_ids().for_each(|id| {
            objects.insert(id, OnceCell::new());
        });

        Self {
            buf,
            xref_table,
            objects,
            encript_key,
        }
    }

    pub fn encript_key(&self) -> Option<&[u8]> {
        self.encript_key.as_deref()
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
            encript_key: None,
        }
    }

    #[cfg(test)]
    pub fn setup_object(&mut self, id: u32, v: Object) {
        self.objects
            .insert(NonZeroU32::new(id).unwrap(), OnceCell::with_value(v));
    }

    pub fn resolve_pdf_object<'b, T: PdfObject<'a, 'b, Self>>(
        &'b self,
        id: NonZeroU32,
    ) -> Result<T, ObjectValueError> {
        let obj = self.resolve(id)?.as_dict()?;
        T::new(Some(id), obj, self)
    }

    /// Resolve object with id `id`, if object is reference, resolve it recursively.
    pub fn resolve(&self, id: NonZeroU32) -> Result<&Object, ObjectValueError> {
        self.objects
            .get(&id)
            .ok_or(ObjectValueError::ObjectIDNotFound(id))?
            .get_or_try_init(|| {
                let mut o = self
                    .xref_table
                    .parse_object(self.buf, id, self.encript_key())?;
                while let Object::Reference(id) = o {
                    o = self
                        .xref_table
                        .parse_object(self.buf, id.id().id(), self.encript_key())?;
                }
                Ok(o)
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
        id: Name,
    ) -> Result<Option<&'c Object>, ObjectValueError> {
        Self::not_found_error_to_opt(self._resolve_container_value(c, id).map(|(_, o)| o))
    }

    /// Resolve value from data container `c` with key `k`, if value is reference,
    /// resolve it recursively.
    pub fn resolve_container_value<'b: 'c, 'c, C: DataContainer>(
        &'b self,
        c: &'c C,
        id: Name,
    ) -> Result<&'c Object, ObjectValueError> {
        self.resolve_required_value(c, id).map(|(_, o)| o)
    }

    /// Like _resolve_container_value(), but error logs if value not exist
    fn resolve_required_value<'b: 'c, 'c, C: DataContainer>(
        &'b self,
        c: &'c C,
        id: Name,
    ) -> Result<(Option<NonZeroU32>, &'c Object), ObjectValueError> {
        self._resolve_container_value(c, id.clone()).map_err(|e| {
            error!("{}: {}", e, id);
            e
        })
    }

    fn _resolve_container_value<'b: 'c, 'c, C: DataContainer>(
        &'b self,
        c: &'c C,
        id: Name,
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
    pub fn resolve_one_or_more_pdf_object<'b, T: PdfObject<'a, 'b, Self>>(
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

impl<'a> Resolver<'a> for ObjectResolver<'a> {
    fn do_resolve_container_value<'b: 'c, 'c, C: DataContainer>(
        &'b self,
        c: &'c C,
        id: Name,
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
    encrypt_key: Option<Box<[u8]>>,
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
) -> AnyResult<Option<Box<[u8]>>> {
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
        name!("Standard"),
        encrypt.filter()?,
        "unsupported security handler"
    );
    assert!(
        encrypt.sub_filter()?.is_none(),
        "unsupported security handler (SubFilter)"
    );
    assert_eq!(Algorithm::Aes, encrypt.algorithm()?);
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
        Ok(Some(calc_encrypt_key(
            encrypt.revison()?,
            encrypt.key_length()? as usize,
            user_password.as_bytes(),
            &owner_hash_arr,
            encrypt.permission_flags()?,
            &trailer.id()?.unwrap().0,
        )))
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
            trailers.iter().find(|d| d.contains_key(&name!("Encrypt"))),
            owner_password,
            user_password,
        )?;

        let root_id = trailers.iter().find_map(|t| t.get(&name!("Root"))).unwrap();
        let root_id = root_id.reference().unwrap().id().id();

        Ok(Self {
            head_ver: head_ver.to_owned(),
            root_id,
            data: buf,
            xref,
            encrypt_key,
        })
    }

    pub fn resolver(&self) -> AnyResult<ObjectResolver<'_>> {
        Ok(ObjectResolver::new(
            &self.data,
            &self.xref,
            self.encrypt_key.clone(),
        ))
    }

    pub fn version<'a>(
        &'a self,
        resolver: &'a ObjectResolver<'a>,
    ) -> Result<String, ObjectValueError> {
        let catalog = self.catalog(resolver)?;
        Ok(catalog
            .ver()
            .map(|s| s.as_ref().to_owned())
            .unwrap_or_else(|| self.head_ver.clone()))
    }

    pub fn catalog<'a, 'b: 'a>(
        &self,
        resolver: &'b ObjectResolver<'a>,
    ) -> Result<Catalog<'a, 'b>, ObjectValueError> {
        Catalog::parse(self.root_id, resolver)
    }
}

/// Read sample file content, panic on any error.
/// `file_path` relate to '~/sample_files/'.
#[cfg(test)]
pub(crate) fn read_sample_file(file_path: impl AsRef<std::path::Path>) -> Vec<u8> {
    use std::{fs::File, io::Read, path::Path};

    let file_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("sample_files")
        .join(file_path);
    let mut buf = Vec::new();
    File::open(file_path)
        .unwrap()
        .read_to_end(&mut buf)
        .unwrap();
    buf
}

/// Decode stream for testing. `file_path` relate to '~/sample_files/'.
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
    let buf = read_sample_file(file_path);
    let f = File::parse(buf, "", "")?;
    let resolver = f.resolver()?;
    let stream = resolver.resolve(id.try_into()?)?.stream()?;
    f_assert(stream.as_dict(), &resolver)?;
    Ok(stream.decode(&resolver)?.into_owned())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod render_tests;
