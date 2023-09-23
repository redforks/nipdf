use bitflags::bitflags;
use pdf2docx_macro::{pdf_object, TryFromIntObjectForBitflags, TryFromNameObject};

use crate::{
    file::Rectangle,
    graphics::{NameOrDictByRef, NameOrStream},
    object::{Object, ObjectValueError, Stream},
};

#[derive(Debug, Copy, Clone, PartialEq, Eq, TryFromNameObject)]
pub enum FontType {
    Type0,
    Type1,
    MMType1,
    Type3,
    TrueType,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, TryFromNameObject)]
pub enum CIDFontType {
    CIDFontType0,
    CIDFontType2,
}

#[pdf_object("Font")]
pub trait FontDictTrait {
    #[try_from]
    fn subtype(&self) -> FontType;

    #[self_as]
    fn type0(&self) -> Type0FontDict<'a, 'b>;

    #[self_as]
    fn type1(&self) -> Type1FontDict<'a, 'b>;

    #[self_as]
    fn truetype(&self) -> TrueTypeFontDict<'a, 'b>;
}

#[pdf_object(("Font", "Type0"))]
pub trait Type0FontDictTrait {
    #[typ("Name")]
    fn base_font(&self) -> &str;
    #[try_from]
    fn encoding(&self) -> NameOrStream<'a, 'b>;
    #[nested]
    fn descendant_fonts(&self) -> Vec<CIDFontDict<'a, 'b>>;
    fn to_unicode(&self) -> Option<&'b Stream<'a>>;
}

/// For standard 14 fonts, font_descriptor/first_char/last_char/widths may not exist.
/// they should all exist or not exist. See PDF32000_2008.pdf page 255
#[pdf_object(("Font", "Type1"))]
pub trait Type1FontDictTrait {
    #[typ("Name")]
    fn base_font(&self) -> &str;
    /// If font is the standard 14 fonts, it may not exist.
    fn first_char(&self) -> Option<u32>;
    /// if font is the standard 14 fonts, it may not exist.
    fn last_char(&self) -> Option<u32>;
    /// if font is the standard 14 fonts, it may not exist.
    fn widths(&self) -> Vec<u32>;
    /// if font is the standard 14 fonts, it may not exist.
    #[nested]
    fn font_descriptor(&self) -> Option<FontDescriptorDict<'a, 'b>>;
    #[try_from]
    fn encoding(&self) -> Option<NameOrDictByRef<'a, 'b>>;
    fn to_unicode(&self) -> Option<&'b Stream<'a>>;
}

#[pdf_object(("Font", "TrueType"))]
pub trait TrueTypeFontDictTrait {
    #[typ("Name")]
    fn base_font(&self) -> &str;
    fn first_char(&self) -> Option<u32>;
    fn last_char(&self) -> Option<u32>;
    fn widths(&self) -> Vec<u32>;
    #[nested]
    fn font_descriptor(&self) -> Option<FontDescriptorDict<'a, 'b>>;
    #[try_from]
    fn encoding(&self) -> Option<NameOrDictByRef<'a, 'b>>;
    fn to_unicode(&self) -> Option<&'b Stream<'a>>;
}

#[derive(Debug, PartialEq, Clone, Copy, strum::AsRefStr)]
pub enum CIDFontEncding {
    IdentityH,
    IdentityV,
}

#[derive(Debug, PartialEq)]
pub enum CIDFontWidthGroup {
    NConsecutive((u32, Vec<u16>)),
    FirstLast { first: u32, last: u32, width: u16 },
}

#[derive(Debug, PartialEq)]
pub struct CIDFontWidths(Vec<CIDFontWidthGroup>);
impl CIDFontWidths {
    pub(crate) fn char_width(&self, ch: u32) -> u32 {
        todo!()
    }
}

impl<'a, 'b> TryFrom<&'b Object<'a>> for CIDFontWidths {
    type Error = ObjectValueError;

    fn try_from(obj: &'b Object<'a>) -> Result<Self, Self::Error> {
        let mut widths = Vec::new();
        let Object::Array(arr) = obj else {
            return Err(Self::Error::UnexpectedType);
        };

        let mut iter = arr.iter();
        while let Some(first) = iter.next() {
            let first = first.as_int()?;
            let second = iter.next().ok_or(Self::Error::UnexpectedType)?;
            match second {
                Object::Array(arr) => {
                    let mut iter = arr.iter();
                    let mut width = Vec::with_capacity(arr.len());
                    while let Some(ref num) = iter.next() {
                        let num = num.as_int()? as u16;
                        width.push(num);
                    }
                    widths.push(CIDFontWidthGroup::NConsecutive((first as u32, width)));
                }
                Object::Integer(last) => {
                    let width = iter.next().ok_or(Self::Error::UnexpectedType)?;
                    widths.push(CIDFontWidthGroup::FirstLast {
                        first: first as u32,
                        last: *last as u32,
                        width: width.as_int()? as u16,
                    });
                }
                _ => return Err(Self::Error::UnexpectedType),
            }
        }
        Ok(CIDFontWidths(widths))
    }
}

#[pdf_object("Font")]
pub trait CIDFontDictTrait {
    #[try_from]
    fn subtype(&self) -> CIDFontType;
    #[typ("Name")]
    fn base_font(&self) -> &str;
    #[nested]
    fn font_descriptor(&self) -> Option<FontDescriptorDict<'a, 'b>>;
    #[default(1000u32)]
    fn dw(&self) -> u32;
    #[try_from]
    fn w(&self) -> CIDFontWidths;
    #[try_from]
    fn cid_to_gid_map(&self) -> Option<NameOrStream<'a, 'b>>;
}

#[pdf_object("FontDescriptor")]
pub trait FontDescriptorDictTrait {
    #[typ("Name")]
    fn font_name(&self) -> &str;

    fn font_family(&self) -> &str;

    #[typ("Name")]
    fn font_stretch(&self) -> Option<&str>;

    fn font_weight(&self) -> Option<u32>;

    #[try_from]
    fn flags(&self) -> FontDescriptorFlags;

    #[try_from]
    fn font_b_box(&self) -> Rectangle;

    fn italic_angle(&self) -> f32;

    fn ascent(&self) -> f32;

    fn descent(&self) -> f32;

    #[or_default]
    fn leading(&self) -> f32;

    fn cap_height(&self) -> Option<f32>;

    #[or_default]
    fn x_height(&self) -> f32;

    fn stem_v(&self) -> f32;

    #[or_default]
    fn stem_h(&self) -> f32;

    #[or_default]
    fn avg_width(&self) -> f32;

    #[or_default]
    fn max_width(&self) -> f32;

    #[or_default]
    fn missing_width(&self) -> u32;

    fn font_file(&self) -> Option<&'b Stream<'a>>;

    fn font_file2(&self) -> Option<&'b Stream<'a>>;

    fn font_file3(&self) -> Option<&'b Stream<'a>>;

    fn char_set(&self) -> Option<&str>;
}

bitflags! {
    #[derive(TryFromIntObjectForBitflags)]
    pub struct FontDescriptorFlags: u32 {
        const FIXED_PITCH = 1;
        const SERIF = 1 << 1;
        const SYMBOLIC = 1 << 2;
        const SCRIPT = 1 << 3;
        const NONSYMBOLIC = 1 << 5;
        const ITALIC = 1 << 6;
        const ALL_CAP = 1 << 16;
        const SMALL_CAP = 1 << 17;
        const FORCE_BOLD = 1 << 18;
    }
}
