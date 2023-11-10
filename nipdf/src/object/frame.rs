use super::{Dictionary, Object, ObjectValueError, XRefSection};
use crate::file::EncryptDict;
use nipdf_macro::pdf_object;
use std::num::NonZeroU32;

/// Document id, two binary string.
pub struct DocId(pub Box<[u8]>, pub Box<[u8]>);

impl<'a> TryFrom<&Object<'a>> for DocId {
    type Error = ObjectValueError;

    fn try_from(o: &Object<'a>) -> Result<Self, Self::Error> {
        let arr = o.as_arr()?;
        if arr.len() != 2 {
            return Err(ObjectValueError::UnexpectedType);
        }

        Ok(Self(arr[0].as_byte_string()?, arr[1].as_byte_string()?))
    }
}

#[pdf_object(())]
pub trait TrailerDictTrait {
    fn size(&self) -> i32;
    fn prev(&self) -> Option<i32>;
    fn root(&self) -> Option<NonZeroU32>;
    #[nested]
    fn encrypt(&self) -> Option<EncryptDict<'a, 'b>>;
    #[key("ID")]
    #[try_from]
    fn id(&self) -> Option<DocId>;
}

#[derive(Debug, Clone)]
/// Frame contains things like xref, trailer, caused by incremental update. See [FrameSet]
pub struct Frame<'a> {
    pub xref_pos: u32,
    pub trailer: Dictionary<'a>,
    pub xref_section: XRefSection,
}

impl<'a> Frame<'a> {
    pub fn new(xref_pos: u32, trailer: Dictionary<'a>, xref_section: XRefSection) -> Self {
        Self {
            xref_pos,
            trailer,
            xref_section,
        }
    }
}

pub type FrameSet<'a> = Vec<Frame<'a>>;
