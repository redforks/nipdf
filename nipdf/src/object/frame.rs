use super::{Dictionary, Object, ObjectValueError, RuntimeObjectId, XRefSection};
use crate::file::EncryptDict;
use nipdf_macro::pdf_object;

/// Document id, two binary string.
pub struct DocId(pub Box<[u8]>, pub Box<[u8]>);

impl TryFrom<&Object> for DocId {
    type Error = ObjectValueError;

    fn try_from(o: &Object) -> Result<Self, Self::Error> {
        let arr = o.as_arr()?;
        if arr.len() != 2 {
            return Err(ObjectValueError::UnexpectedType);
        }

        Ok(Self(arr[0].as_bstr()?.into(), arr[1].as_bstr()?.into()))
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
/// Frame contains things like xref, trailer, caused by incremental update. See [FrameSet]
pub struct Frame {
    pub trailer: Dictionary,
    pub xref_section: XRefSection,
}

impl Frame {
    pub fn new(trailer: Dictionary, xref_section: XRefSection) -> Self {
        Self {
            trailer,
            xref_section,
        }
    }
}

pub type FrameSet = Vec<Frame>;
