use super::{Dictionary, Entry, Object, ObjectValueError, ObjectWithResolver, RuntimeObjectId};
use crate::file::EncryptDict;
use nipdf_macro::pdf_object;

/// Document id, two binary string.
pub struct DocId(pub Box<[u8]>, pub Box<[u8]>);

impl TryFrom<ObjectWithResolver<'_, '_>> for DocId {
    type Error = ObjectValueError;

    fn try_from(o: ObjectWithResolver<'_, '_>) -> Result<Self, Self::Error> {
        let arr = o.into_schema_array()?;
        if arr.len() != 2 {
            return Err(ObjectValueError::UnexpectedType);
        }

        Ok(Self(
            arr.required_object(0)?.as_bstr()?.into(),
            arr.required_object(1)?.as_bstr()?.into(),
        ))
    }
}

#[pdf_object(())]
pub trait TrailerDictTrait {
    fn size(&self) -> i32;
    fn prev(&self) -> Option<i32>;
    fn root(&self) -> Option<RuntimeObjectId>;
    #[nested]
    fn encrypt(&self) -> Option<EncryptDict<'a, 'b>>;
    #[key("ID")]
    #[try_from]
    fn id(&self) -> Option<DocId>;
}

#[derive(Debug, PartialEq, Clone)]
/// Frame contains things like xref, trailer, caused by incremental update.
pub struct Frame {
    pub trailer: Dictionary,
    pub xref_section: Vec<(u32, Entry)>,
}

impl Frame {
    pub fn new(trailer: Dictionary, xref_section: Vec<(u32, Entry)>) -> Self {
        Self {
            trailer,
            xref_section,
        }
    }
}
