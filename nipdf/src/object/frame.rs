use super::{Dictionary, Object, ObjectValueError, XRefSection};
use crate::file::EncryptDict;
use nipdf_macro::pdf_object;
use std::num::NonZeroU32;

/// Document id, two binary string.
pub struct DocId(pub Box<[u8]>, pub Box<[u8]>);

impl TryFrom<&Object> for DocId {
    type Error = ObjectValueError;

    fn try_from(o: &Object) -> Result<Self, Self::Error> {
        let arr = o.arr()?;
        if arr.len() != 2 {
            return Err(ObjectValueError::UnexpectedType);
        }

        Ok(Self(
            arr[0].as_byte_string()?.into(),
            arr[1].as_byte_string()?.into(),
        ))
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
pub struct Frame {
    pub xref_pos: u32,
    pub trailer: Dictionary,
    pub xref_section: XRefSection,
}

impl Frame {
    pub fn new(xref_pos: u32, trailer: Dictionary, xref_section: XRefSection) -> Self {
        Self {
            xref_pos,
            trailer,
            xref_section,
        }
    }
}

pub type FrameSet<'a> = Vec<Frame>;
