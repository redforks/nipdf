use pdf2docx_macro::{pdf_object, TryFromNameObject};

use crate::{graphics::NameOrStream, object::Stream};

#[derive(Debug, Copy, Clone, PartialEq, Eq, TryFromNameObject)]
pub enum FontType {
    Type0,
    Type1,
    MMType1,
    Type3,
    TrueType,
    CIDFontType0,
    CIDFontType2,
}

#[pdf_object("Font")]
pub(crate) trait FontDictTrait {
    #[try_from]
    fn subtype(&self) -> FontType;

    #[self_as]
    fn type0(&self) -> Type0FontDict<'a, 'b>;
}

#[pdf_object(("Font", "Type0"))]
pub(crate) trait Type0FontDictTrait {
    #[typ("Name")]
    fn base_font(&self) -> &str;
    #[try_from]
    fn encoding(&self) -> NameOrStream<'a, 'b>;
    #[nested]
    fn descendant_fonts(&self) -> Vec<FontDict<'a, 'b>>;
    fn to_unicode(&self) -> Option<&'b Stream<'a>>;
}
