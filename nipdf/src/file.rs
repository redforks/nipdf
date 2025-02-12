//! Contains types of PDF file structures.

use crate::{
    ObjectResolveSnafu, ObjectValueError, Result,
    file::encrypt::Authorizer,
    object::{
        Array, Dictionary, Embedded, Entry, Frame, HexString, LiteralString, Object, ObjectId,
        PdfObject, Root, RootPdfObject, RuntimeObjectId, Stream, TrailerDict,
    },
    parser::{
        self, header_parser, indirect_object_def, object_id, parse_frame_set, wsc_prefixed0, wsc0,
        wsc1,
    },
};
use ahash::{HashMap, HashMapExt};
use either::Either;
use log::error;
use nipdf_macro::pdf_object;
use once_cell::unsync::OnceCell;
use prescript::{Name, ParserError, sname};
use snafu::{OptionExt as _, Report, ResultExt as _, Snafu, ensure_whatever, whatever};
use std::iter::repeat_with;
use winnow::{
    LocatingSlice, ModalResult, Parser as _,
    combinator::{alt, repeat, terminated},
    error::{ContextError, ErrMode, ParseError},
    stream::{Compare, StreamIsPartial},
    token::{any, rest, take_until},
};

pub mod page;
pub use page::*;

pub(crate) mod encrypt;

use self::encrypt::{CryptFilters, VecLike};
pub use encrypt::EncryptDict;

#[derive(Debug, Copy, Clone)]
pub enum ObjectPos {
    Offset(u32),
    InStream(RuntimeObjectId, u16),
}

impl<'a> From<&'a Entry> for ObjectPos {
    fn from(e: &'a Entry) -> Self {
        match e {
            Entry::InFile(pos) => ObjectPos::Offset(pos.offset()),
            Entry::InStream(id, idx) => ObjectPos::InStream(*id, *idx),
        }
    }
}

/// Object stream stores multiple objects in a stream. See section 7.5.7
#[derive(Debug)]
struct ObjectStream {
    /// Data contains all objects in this stream, without index part.
    buf: Vec<u8>,
    /// offsets of objects in `buf`
    offsets: Vec<u16>,
}

// TODO: use and create test
fn object_stream_parser<'a, S>(n: usize) -> impl winnow::Parser<S, ObjectStream, ContextError> + 'a
where
    S: winnow::stream::Stream<Token = u8, Slice = &'a [u8]> + StreamIsPartial + Compare<u8> + 'a,
{
    use winnow::{
        ascii::{dec_uint, space1},
        combinator::{preceded, repeat, terminated},
    };
    (
        repeat(
            n,
            terminated(
                preceded((dec_uint::<_, u32, _>, space1), dec_uint::<_, u16, _>),
                wsc0(),
            ),
        ),
        rest,
    )
        .map(|(nums, buf): (Vec<u16>, &'a [u8])| ObjectStream {
            buf: buf.to_owned(),
            offsets: nums,
        })
}

impl ObjectStream {
    pub fn new(
        stream: &Stream,
        file: &[u8],
        encrypt_info: Option<&EncryptInfo>,
    ) -> Result<Self, ObjectValueError> {
        let d = stream.as_dict();
        ensure_whatever!(
            &sname("ObjStm") == d[&sname("Type")].as_name()?,
            "not object stream"
        );
        let n = d.get(&sname("N")).map_or(Ok(0), Object::int)? as usize;
        let buf = stream.decode_without_resolve_length(file, encrypt_info)?;
        let r = object_stream_parser(n).parse(buf.as_ref())?;
        Ok(r)
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
    id_offset: HashMap<RuntimeObjectId, ObjectPos>,
    // object id -> offset
    object_streams: HashMap<RuntimeObjectId, OnceCell<ObjectStream>>, // stream id -> ObjectStream
}

impl XRefTable {
    pub fn new(id_offset: HashMap<RuntimeObjectId, ObjectPos>) -> Self {
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
            id_offset: HashMap::default(),
            object_streams: HashMap::new(),
        }
    }

    /// Scan IDOffsetMap by scan indirect object declaration,
    /// helps to create pdf file objects for testing.
    pub fn from_buf(buf: &[u8]) -> Result<Self> {
        use winnow::combinator::{repeat, terminated};

        let objects: Vec<_> = terminated(
            repeat(1.., wsc_prefixed0(indirect_object_def::<_, ParserError>())),
            wsc0(),
        )
        .parse(LocatingSlice::new(buf))
        .map_err(ParseError::into_inner)
        .whatever_context::<_, ObjectValueError>("parse xref table objects")?;
        let mut id_offset = HashMap::new();
        for o in objects {
            let search_key = format!("{} {} obj", o.to_runtime_object_id(), o.id().generation());
            let pos: u32 = buf
                .windows(search_key.len())
                .position(|w| w == search_key.as_bytes())
                .whatever_context::<_, ObjectValueError>("get object position")?
                .try_into()
                .whatever_context::<_, ObjectValueError>("convert position into u32")?;
            id_offset.insert(o.into(), ObjectPos::Offset(pos));
        }

        Ok(Self::new(id_offset))
    }

    fn scan(frame_set: &[Frame]) -> HashMap<RuntimeObjectId, ObjectPos> {
        let mut r = HashMap::with_capacity(5000);
        for (id, entry) in frame_set.iter().rev().flat_map(|f| f.xref_section.iter()) {
            if entry.is_used() {
                r.insert(RuntimeObjectId(*id), entry.into());
            } else if *id != 0 {
                r.remove(id);
            }
        }
        r
    }

    pub fn from_frame_set(frame_set: &[Frame]) -> Self {
        Self::new(Self::scan(frame_set))
    }

    /// Return `buf` start from where `id` is. Return Left if object is direct stored in file,
    /// return Right if object is in object stream.
    fn resolve_object_buf<'a, 'b>(
        &'b self,
        buf: &'a [u8],
        id: impl Into<RuntimeObjectId>,
        encrypt_info: Option<&EncryptInfo>,
    ) -> Result<Either<&'a [u8], &'b [u8]>, ObjectValueError> {
        fn parse_indirect_stream(input: &[u8]) -> Result<Stream, ObjectValueError> {
            let (_, o) = indirect_object_def::<_, ContextError<&'static str>>()
                .parse_peek(LocatingSlice::new(input))?;
            let Object::Stream(s) = o.take() else {
                whatever!("expected stream");
            };
            Ok(s)
        }

        let id = id.into();

        // Look up the entry in id_offset map
        let entry = self
            .id_offset
            .get(&id)
            .ok_or(ObjectValueError::ObjectIDNotFound { id })?;

        // Match on the entry type and return appropriate buffer
        match entry {
            ObjectPos::Offset(offset) => Ok(Either::Left(&buf[*offset as usize..])),
            ObjectPos::InStream(id, idx) => {
                let object_stream = self.object_streams[id].get_or_try_init(|| {
                    let obj_buf = self
                        .resolve_object_buf(buf, *id, encrypt_info)?
                        .left()
                        .whatever_context::<_, ObjectValueError>(
                        "object stream should not be in another object stream",
                    )?;

                    let mut stream = parse_indirect_stream(obj_buf)
                        .whatever_context::<_, ObjectValueError>("parse indirect stream")?;

                    let length = stream.0.get("Length").cloned();
                    if let Some(Object::Reference(id)) = length {
                        let v = self
                            .parse_object(buf, id, None)
                            .whatever_context::<_, ObjectValueError>("parse object")?;
                        stream.0.update(|d| {
                            d.insert(sname("Length"), v);
                        });
                    }

                    ObjectStream::new(&stream, obj_buf, encrypt_info)
                })?;

                Ok(Either::Right(object_stream.get_buf(*idx as usize)))
            }
        }
    }

    pub fn parse_object<'a: 'c, 'b: 'c, 'c>(
        &'b self,
        buf: &'a [u8],
        id: impl Into<RuntimeObjectId>,
        encrypt_info: Option<&EncryptInfo>,
    ) -> Result<Object, ObjectValueError> {
        let id = id.into();
        self.resolve_object_buf(buf, id, encrypt_info)
            .and_then(|buf| {
                buf.either(
                    |buf| {
                        terminated(
                            indirect_object_def::<_, ParserError>().try_map(|o| {
                                let id = o.id();
                                let o = o.take();
                                if let Some(encrypt_info) = encrypt_info {
                                    decrypt_string(encrypt_info, id, o)
                                } else {
                                    Ok(o)
                                }
                            }),
                            rest,
                        )
                        .parse(LocatingSlice::new(buf))
                        .map_err(ParseError::into_inner)
                        .with_whatever_context(|_| format!("parse object {}", id))
                    },
                    |buf| {
                        terminated(
                            parser::object::<_, ParserError>(),
                            /* some invalid pdf file contains endobj after data,
                             * pdf.js/test/pdfs/bug1037816.pdf, use rest to ignore it */
                            rest,
                        )
                        .parse(buf)
                        .map_err(ParseError::into_inner)
                        .with_whatever_context(|_| format!("parse inside stream object {}", id))
                    },
                )
            })
    }

    pub fn iter_ids(&self) -> impl Iterator<Item = RuntimeObjectId> + '_ {
        self.id_offset.keys().copied()
    }

    pub fn count(&self) -> usize {
        self.id_offset.len()
    }
}

/// Decrypt HexString/LiteralString nested in object.
fn decrypt_string(encrypt_info: &EncryptInfo, id: ObjectId, mut o: Object) -> Result<Object> {
    struct Decryptor<'a>(&'a EncryptInfo, ObjectId);

    impl Decryptor<'_> {
        fn hex_string(&self, s: &mut HexString) -> Result<()> {
            self.0.string_decrypt(self.1, &mut s.0)
        }

        fn literal_string(&self, s: &mut LiteralString) -> Result<()> {
            self.0.string_decrypt(self.1, &mut s.0)
        }

        fn dict(&self, dict: &mut Dictionary) -> Result<()> {
            dict.update(|d| {
                for (_, v) in d.iter_mut() {
                    self.decrypt(v)?;
                }
                Ok(())
            })
        }

        fn arr(&self, arr: &mut Array) -> Result<()> {
            Object::try_update_array_items(arr, |o| self.decrypt(o))
        }

        fn stream(&self, stream: &mut Stream) -> Result<()> {
            self.dict(&mut stream.0)
        }

        fn decrypt(&self, o: &mut Object) -> Result<()> {
            match o {
                Object::HexString(s) => self.hex_string(s),
                Object::LiteralString(s) => self.literal_string(s),
                Object::Dictionary(d) => self.dict(d),
                Object::Array(arr) => self.arr(arr),
                Object::Stream(s) => self.stream(s),
                _ => Ok(()),
            }
        }
    }

    Decryptor(encrypt_info, id).decrypt(&mut o)?;
    Ok(o)
}

#[derive(Clone)]
pub struct EncryptInfo {
    encrypt_key: Box<[u8]>,
    filters: CryptFilters,
}

impl EncryptInfo {
    pub fn new(encrypt_key: Box<[u8]>, filters: CryptFilters) -> Self {
        Self {
            encrypt_key,
            filters,
        }
    }

    pub fn stream_decrypt(
        &self,
        filter: Option<Name>,
        id: ObjectId,
        data: &mut Vec<u8>,
    ) -> Result<()> {
        self.filters
            .stream_filter(filter)
            .decrypt(&self.encrypt_key, id, data)
    }

    pub fn string_decrypt(&self, id: ObjectId, data: &mut impl VecLike) -> Result<()> {
        self.filters
            .string_filter()
            .decrypt(&self.encrypt_key, id, data)
    }
}

pub trait ObjectKind {}

impl ObjectKind for Root {}
impl ObjectKind for Embedded {}

/// Object impl this trait to resolve from ObjectResolver.
pub trait RootObjectResolveable<'a, 'b>
where
    Self: Sized + 'a + 'b,
{
    fn resolve(
        resolver: &'b ObjectResolver<'a>,
        id: RuntimeObjectId,
    ) -> Result<Self, ObjectValueError>;
}

impl<'a, 'b, T> RootObjectResolveable<'a, 'b> for (T, Root)
where
    T: RootPdfObject<'a, 'b> + 'b + 'a,
{
    fn resolve(
        resolver: &'b ObjectResolver<'a>,
        id: RuntimeObjectId,
    ) -> Result<Self, ObjectValueError> {
        let o = resolver.resolve(id)?;
        Ok((T::new(id, o.as_dict()?, resolver)?, Root))
    }
}

impl<'a, 'b, T> RootObjectResolveable<'a, 'b> for (T, Embedded)
where
    T: PdfObject<'a, 'b> + 'b + 'a,
{
    fn resolve(
        resolver: &'b ObjectResolver<'a>,
        id: RuntimeObjectId,
    ) -> Result<Self, ObjectValueError> {
        let o = resolver.resolve(id)?;
        Ok((T::new(o.as_dict()?, resolver)?, Embedded))
    }
}

pub struct ObjectResolver<'a> {
    buf: &'a [u8],
    xref_table: &'a XRefTable,
    objects: HashMap<RuntimeObjectId, OnceCell<Object>>,
    encrypt_info: Option<EncryptInfo>,
}

impl<'a> ObjectResolver<'a> {
    pub fn new(
        buf: &'a [u8],
        xref_table: &'a XRefTable,
        encrypt_info: Option<EncryptInfo>,
    ) -> Self {
        let mut objects = HashMap::with_capacity(xref_table.count());
        xref_table.iter_ids().for_each(|id| {
            objects.insert(id, OnceCell::new());
        });

        Self {
            buf,
            xref_table,
            objects,
            encrypt_info,
        }
    }

    pub fn encrypt_info(&self) -> Option<&EncryptInfo> {
        self.encrypt_info.as_ref()
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
            encrypt_info: None,
        }
    }

    #[cfg(test)]
    pub fn setup_object(&mut self, id: impl Into<RuntimeObjectId>, v: Object) {
        self.objects.insert(id.into(), OnceCell::with_value(v));
    }

    /// Resolve an object by ID, caching it in “objects”. If not in XRef, returns an error.
    ///
    /// Returns [ObjectValueError::ObjectResolveError] if resolve object failed.
    /// Returns [ObjectValueError::ObjectIDNotFound] if object not found.
    pub fn resolve(&self, id: impl Into<RuntimeObjectId>) -> Result<&Object, ObjectValueError> {
        let id = id.into();
        self.objects
            .get(&id)
            .ok_or(ObjectValueError::ObjectIDNotFound { id })?
            .get_or_try_init(|| {
                self.xref_table
                    .parse_object(self.buf, id, self.encrypt_info())
                    .context(ObjectResolveSnafu)
            })
    }

    /// If the given Object is a reference, resolve that reference; otherwise return the same
    /// Object.
    pub fn resolve_reference<'b>(&'b self, v: &'b Object) -> Result<&'b Object, ObjectValueError> {
        if let Object::Reference(id) = v {
            self.resolve(id)
        } else {
            Ok(v)
        }
    }

    /// If `o` is dict or stream, construct PdfObject from it, resolve it if `o` is reference.
    pub fn as_pdf_object<'b, T>(&'b self, o: &'b Object) -> Result<T, ObjectValueError>
    where
        T: PdfObject<'a, 'b>,
    {
        let o = self.resolve_reference(o)?;
        let dict = o.as_dict()?;
        PdfObject::new(dict, self)
    }

    /// Generic “resolve by ID” API that uses the “RootObjectResolveable” trait, plus the
    /// appropriate K marker type (Root or Embedded). This dispatches to the trait’s “resolve”.
    pub fn resolve_pdf_object<'b, T, K>(
        &'b self,
        id: impl Into<RuntimeObjectId>,
    ) -> Result<T, ObjectValueError>
    where
        (T, K): RootObjectResolveable<'a, 'b>,
        K: ObjectKind,
    {
        Ok(<(T, _)>::resolve(self, id.into())?.0)
    }

    /// Return the raw file bytes for a “stream” object, starting at the specified ID’s offset
    /// in the PDF file. This is primarily for parser usage. If the object is an “InStream”,
    /// you’ll get Right(...) from xref_table.resolve_object_buf(), so we need Left(...) here.
    pub fn stream_data(
        &self,
        id: impl Into<RuntimeObjectId>,
    ) -> Result<&'a [u8], ObjectValueError> {
        self.xref_table
            .resolve_object_buf(self.buf, id, self.encrypt_info())?
            .left()
            .whatever_context("stream should not in ObjectStream")
    }
}

#[pdf_object("Catalog")]
#[root_pdf_object]
trait CatalogDictTrait {
    fn version(&self) -> Option<Name>;
    #[nested]
    fn pages(&self) -> PageDict<'a, 'b>;
}

#[derive(Debug)]
pub struct Catalog<'a> {
    d: CatalogDict<'a, 'a>,
}

impl<'a> Catalog<'a> {
    fn parse(
        id: impl Into<RuntimeObjectId>,
        resolver: &'a ObjectResolver<'a>,
    ) -> Result<Self, ObjectValueError> {
        Ok(Self {
            d: resolver
                .resolve_pdf_object(id)
                .whatever_context::<_, ObjectValueError>("resolve catalog")?,
        })
    }

    pub fn pages(&self) -> Result<Vec<Page<'a>>> {
        Page::parse(
            self.d
                .pages()
                .whatever_context::<_, ObjectValueError>("resolve pages")?,
        )
    }

    pub fn ver(&self) -> Option<Name> {
        self.d.version().unwrap_or_else(|e| {
            log::warn!("Failed to get version {}", e);
            None
        })
    }
}

pub struct File {
    root_id: RuntimeObjectId,
    head_ver: Option<String>,
    data: Vec<u8>,
    xref: XRefTable,
    encrypt_info: Option<EncryptInfo>,
}

#[derive(Debug, Clone, Copy, Snafu)]
pub enum FileError {
    #[snafu(display("catalog object is required"))]
    CatalogRequired,
    #[snafu(display("missing required trailer value"))]
    MissingRequiredTrailerValue,
    #[snafu(display("invalid password"))]
    InvalidPassword,
    #[snafu(display("invalid file"))]
    InvalidFile,
}

impl From<ObjectValueError> for FileError {
    fn from(e: ObjectValueError) -> Self {
        error!("object value error on open file: {}", e);
        Self::InvalidFile
    }
}

/// Open possible encrypt file, return None if not encrypted.
fn open_encrypt(
    buf: &[u8],
    xref: &XRefTable,
    trailer: Option<&Dictionary>,
    password: &str,
) -> Result<Option<EncryptInfo>> {
    let Some(trailer) = trailer else {
        return Ok(None);
    };

    let resolver = ObjectResolver::new(buf, xref, None);
    let trailer = TrailerDict::new(trailer, &resolver)
        .whatever_context::<_, ObjectValueError>("parse trailer dict")?;
    let encrypt = trailer
        .encrypt()
        .map_err(|e| {
            drop(e);
            FileError::InvalidFile
        })
        .whatever_context::<_, ObjectValueError>("parse encrypt dict")?;
    let Some(encrypt) = encrypt else {
        return Ok(None);
    };

    ensure_whatever!(
        sname("Standard")
            == encrypt
                .filter()
                .whatever_context::<_, ObjectValueError>("get encrypt filter")?,
        "unsupported security handler"
    );
    ensure_whatever!(
        encrypt
            .sub_filter()
            .whatever_context::<_, ObjectValueError>("get encrypt sub filter")?
            .is_none(),
        "unsupported security handler (SubFilter)"
    );

    let authorizer = Authorizer::new(&encrypt, &trailer)
        .whatever_context::<_, ObjectValueError>("get authorizer info")?;

    let k = authorizer
        .authorize(password.as_bytes())
        .ok_or(FileError::InvalidPassword)
        .whatever_context::<_, ObjectValueError>("check password")?;
    Ok(Some(EncryptInfo::new(k, encrypt.crypt_filters()?)))
}

fn index_xref<'a, E>(
    data: &mut LocatingSlice<&'a [u8]>,
) -> ModalResult<(Vec<usize>, Vec<(ObjectId, usize)>), E>
where
    E: winnow::error::ParserError<LocatingSlice<&'a [u8]>> + 'a,
{
    const TRAILER_BYTES: &[u8] = b"trailer";
    let mut trailer_positions = Vec::new();
    let mut entries = Vec::new();

    let parse_trailer_position_after_xref = (b"xref".as_slice(), take_until(1.., TRAILER_BYTES))
        .span()
        .map(|r| trailer_positions.push(r.end));
    let parse_indirect_object =
        (object_id().with_span(), wsc1(), b"obj".as_slice()).map(|((obj_id, range), ..)| {
            entries.push((obj_id, range.start));
        });
    let mut parser = repeat::<_, _, (), _, _>(
        1..,
        alt((
            wsc1().void(),
            parse_indirect_object,
            // fix: prevent match "startxref" by parse_trailer_position_after_xref()
            b"startxref".as_slice().void(),
            parse_trailer_position_after_xref,
            any.void(),
        )),
    );
    parser.parse_next(data)?;
    drop(parser);

    Ok((trailer_positions, entries))
}

fn parse_header(buf: &[u8]) -> Option<String> {
    match header_parser().parse_next(&mut &buf[..]) {
        Ok(ver) => Some(ver.to_owned()),
        Err(e) => {
            log::warn!("Failed to parse header: {}", e);
            None
        }
    }
}

fn get_root_id(trailers: &[Dictionary]) -> Result<RuntimeObjectId> {
    let root_id = trailers
        .iter()
        .find_map(|t| t.get(&sname("Root")))
        .whatever_context::<_, ObjectValueError>("Root entry not found in trailers")?;
    let root_id = root_id
        .reference()
        .whatever_context::<_, ObjectValueError>("Failed to get reference from root_id")?
        .id()
        .id();
    Ok(root_id)
}

impl File {
    fn normal_parse(buf: Vec<u8>, user_password: &str) -> Result<Self> {
        let head_ver = parse_header(&buf);

        let frame_set = parse_frame_set::<ParserError>
            .parse(&buf[..])
            .map_err(ParseError::into_inner)
            .whatever_context::<_, ObjectValueError>("parse frame set")?;
        let xref = XRefTable::from_frame_set(&frame_set);

        let trailers: Vec<_> = frame_set.into_iter().map(|f| f.trailer).collect();
        let encrypt_key = open_encrypt(
            &buf,
            &xref,
            trailers.iter().find(|d| d.contains_key(&sname("Encrypt"))),
            user_password,
        )?;

        Ok(Self {
            head_ver,
            root_id: get_root_id(&trailers)?,
            data: buf,
            xref,
            encrypt_info: encrypt_key,
        })
    }

    pub fn parse(buf: Vec<u8>, user_password: &str) -> Result<Self> {
        // Try normal parsing first
        match Self::normal_parse(buf.clone(), user_password) {
            Ok(file) => Ok(file),
            Err(e) => {
                // Log the normal parse error
                log::warn!(
                    "Normal PDF parsing failed, attempting rebuild: {}",
                    Report::from_error(e)
                );

                // Fallback to rebuilding xref table
                Self::build_xref(buf, user_password)
            }
        }
    }

    /// Do not use xref/trailer/encrypt info from file, scan them from buf.
    ///
    /// Used for corruptted xref/trailer/encrypt pdf file.
    pub fn build_xref(buf: Vec<u8>, user_password: &str) -> Result<Self> {
        let head_ver = parse_header(&buf);

        // Scan the file for trailer positions and object entries
        let (trailer_positions, object_entries) = index_xref::<ParserError>
            .parse(LocatingSlice::new(&buf))
            .map_err(ParseError::into_inner)
            .whatever_context::<_, ObjectValueError>("scan file for xref entries")?;

        // Build id_offset map from object entries
        let mut id_offset = HashMap::with_capacity(object_entries.len());
        for (obj_id, offset) in object_entries {
            id_offset.insert(
                RuntimeObjectId(obj_id.id().0),
                ObjectPos::Offset(
                    offset
                        .try_into()
                        .whatever_context::<_, ObjectValueError>("convert offset")?,
                ),
            );
        }

        // Create XRef table
        let xref = XRefTable::new(id_offset);

        // Parse trailers
        let mut trailers = Vec::with_capacity(trailer_positions.len());
        for pos in trailer_positions {
            if let Ok((_, _, dict)) = terminated(
                (
                    b"trailer".as_slice(),
                    wsc0::<_, ErrMode<ParserError>>(),
                    parser::dict,
                ),
                rest,
            )
            .parse(&buf[pos..])
            {
                trailers.push(dict);
            }
        }

        // Get encrypt info if file is encrypted
        let encrypt_key = open_encrypt(
            &buf,
            &xref,
            trailers.iter().find(|d| d.contains_key(&sname("Encrypt"))),
            user_password,
        )?;

        Ok(Self {
            head_ver,
            root_id: get_root_id(&trailers)?,
            data: buf,
            xref,
            encrypt_info: encrypt_key,
        })
    }

    /// Return file data by consuming self.
    ///
    /// Used for corruptted xref/trailer/encrypt pdf file to rebuild file with `Self::build_xref()`.
    pub fn into_data(self) -> Vec<u8> {
        self.data
    }

    pub fn resolver(&self) -> Result<ObjectResolver<'_>> {
        Ok(ObjectResolver::new(
            &self.data,
            &self.xref,
            self.encrypt_info.clone(),
        ))
    }

    pub fn version<'a>(
        &'a self,
        resolver: &'a ObjectResolver<'a>,
    ) -> Result<Option<String>, ObjectValueError> {
        let catalog = self.catalog(resolver)?;
        Ok(catalog
            .ver()
            .map_or_else(|| self.head_ver.clone(), |s| Some(s.into_string())))
    }

    pub fn catalog<'a>(
        &self,
        resolver: &'a ObjectResolver<'a>,
    ) -> Result<Catalog<'a>, ObjectValueError> {
        Catalog::parse(self.root_id, resolver)
    }
}

/// Decode stream for testing. `file_path` relate to current crate directory.
/// `f_assert` called with `Dictionary` of stream to do some test on it.
#[cfg(test)]
pub(crate) fn decode_stream<
    E: std::error::Error + Sync + Send + 'static,
    T: TryInto<u32, Error = E>,
>(
    file_path: impl AsRef<std::path::Path>,
    id: T,
    f_assert: impl for<'a> FnOnce(&'a Dictionary, &'a ObjectResolver<'a>) -> Result<()>,
) -> Result<Vec<u8>> {
    use snafu::ResultExt;

    let f = open_test_file(file_path);
    let resolver = f.resolver()?;
    let stream = resolver
        .resolve(
            id.try_into()
                .whatever_context::<_, ObjectValueError>("convert id")?,
        )
        .whatever_context::<_, ObjectValueError>("resolve object")?
        .as_stream()
        .whatever_context::<_, ObjectValueError>("resolve stream")?;
    f_assert(stream.as_dict(), &resolver)?;
    Ok(stream
        .decode(&resolver)
        .whatever_context::<_, ObjectValueError>("decode stream")?
        .into_owned())
}

#[cfg(test)]
pub(crate) fn test_file(file_path: impl AsRef<std::path::Path>) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file_path)
}

/// Open file for testing. `file_path` relate to current crate directory.
#[cfg(test)]
pub(crate) fn open_test_file(file_path: impl AsRef<std::path::Path>) -> File {
    let file_path = test_file(file_path);
    let data = std::fs::read(file_path).unwrap();
    File::parse(data, "").unwrap()
}

#[cfg(test)]
pub(crate) fn open_test_file_with_password(
    file_path: impl AsRef<std::path::Path>,
    p: &str,
) -> Result<File> {
    let file_path = test_file(file_path);
    let data = std::fs::read(file_path).whatever_context::<_, ObjectValueError>("read file")?;
    File::parse(data, p)
}

#[cfg(test)]
pub(crate) fn report_parse_err<I, T, E: std::error::Error>(rv: Result<T, ParseError<I, E>>) -> T {
    use snafu::Report;

    rv.map_err(|e| Report::from_error(e.into_inner())).unwrap()
}

#[cfg(test)]
pub(crate) fn report_peek_err<T, E: std::error::Error>(rv: ModalResult<T, E>) -> T {
    use snafu::Report;

    rv.map_err(|e| Report::from_error(e.into_inner().unwrap()))
        .unwrap()
}

#[cfg(test)]
mod tests;
