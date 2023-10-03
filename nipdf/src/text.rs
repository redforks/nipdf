use std::{collections::HashMap, convert::AsRef};

use bitflags::bitflags;
use nipdf_macro::{pdf_object, TryFromIntObjectForBitflags, TryFromNameObject};

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
    fn first_char(&self) -> u32;
    fn last_char(&self) -> u32;
    fn widths(&self) -> Vec<u32>;
    #[nested]
    fn font_descriptor(&self) -> Option<FontDescriptorDict<'a, 'b>>;
    #[try_from]
    fn encoding(&self) -> Option<NameOrDictByRef<'a, 'b>>;
    fn to_unicode(&self) -> Option<&'b Stream<'a>>;
}

#[derive(Debug, PartialEq)]
pub enum CIDFontWidthGroup {
    NConsecutive((u32, Vec<u16>)),
    FirstLast { first: u32, last: u32, width: u16 },
}

#[derive(Debug, PartialEq)]
pub struct CIDFontWidths(Vec<CIDFontWidthGroup>);
impl CIDFontWidths {
    /// Return None if ch out of range
    pub(crate) fn char_width(&self, ch: u32) -> Option<u32> {
        for group in &self.0 {
            match group {
                CIDFontWidthGroup::NConsecutive((first, widths)) => {
                    if ch >= *first && ch < *first + widths.len() as u32 {
                        return Some(widths[(ch - first) as usize] as u32);
                    }
                }
                CIDFontWidthGroup::FirstLast { first, last, width } => {
                    if ch >= *first && ch <= *last {
                        return Some(*width as u32);
                    }
                }
            }
        }
        None
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
                    let mut width = Vec::with_capacity(arr.len());
                    for num in arr.iter() {
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
    fn font_name(&self) -> &'b str;

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

/// Map to pdf Encoding object Differences field. Override character code
/// to glyph names from BaseEncoding.
pub struct EncodingDifferences<'a>(HashMap<u8, &'a str>);

/// Parse Differences field in Encoding object, which is an array of
/// character code and one or several glyph names. First name is mapped
/// to character code, second name is mapped to character code + 1, and so on.
impl<'a, 'b> TryFrom<&'b Object<'a>> for EncodingDifferences<'b> {
    type Error = ObjectValueError;

    fn try_from(obj: &'b Object<'a>) -> Result<Self, Self::Error> {
        let mut map = HashMap::new();
        let Object::Array(arr) = obj else {
            return Err(Self::Error::UnexpectedType);
        };

        let mut iter = arr.iter();
        let Some(o) = iter.next() else {
            return Ok(EncodingDifferences(map));
        };

        let mut code = o.as_int()?;
        for o in iter {
            match o {
                Object::Name(name) => {
                    map.insert(code as u8, name.as_ref());
                    code += 1;
                }
                Object::Integer(num) => {
                    code = *num;
                }
                _ => return Err(Self::Error::UnexpectedType),
            };
        }
        Ok(EncodingDifferences(map))
    }
}

/// Encoding object for Non Type0 and Type3 fonts
#[pdf_object(Some("Encoding"))]
pub trait EncodingDictTrait {
    #[typ("Name")]
    fn base_encoding(&self) -> Option<&str>;

    #[try_from]
    fn differences(&self) -> Option<EncodingDifferences<'b>>;
}

/// Encoding for Type1
/// map char code (u8) to glyph name
#[derive(Debug)]
pub struct Encoding<'a>([Option<&'a str>; 256]);

impl<'a> Encoding<'a> {
    pub fn decode(&self, ch: u8) -> Option<&'a str> {
        self.0[ch as usize]
    }

    pub fn apply_differences(&self, diff: &EncodingDifferences<'a>) -> Self {
        let mut new = self.0;
        for (ch, name) in diff.0.iter() {
            new[*ch as usize] = Some(*name);
        }
        Self(new)
    }
}

impl Encoding<'static> {
    pub fn predefined(name: &str) -> Option<Self> {
        match name {
            "MacRomanEncoding" => Some(Self::MAC_ROMAN),
            "MacExpertEncoding" => Some(Self::MAC_EXPORT),
            "WinAnsiEncoding" => Some(Self::WIN_ANSI),
            "StandardEncoding" => Some(Self::STANDARD),
            _ => None,
        }
    }

    pub const STANDARD: Self = Self([
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some("space"),
        Some("exclam"),
        Some("quotedbl"),
        Some("numbersign"),
        Some("dollar"),
        Some("percent"),
        Some("ampersand"),
        Some("quotesingle"),
        Some("parenleft"),
        Some("parenright"),
        Some("asterisk"),
        Some("plus"),
        Some("comma"),
        Some("hyphen"),
        Some("period"),
        Some("slash"),
        Some("zero"),
        Some("one"),
        Some("two"),
        Some("three"),
        Some("four"),
        Some("five"),
        Some("six"),
        Some("seven"),
        Some("eight"),
        Some("nine"),
        Some("colon"),
        Some("semicolon"),
        Some("less"),
        Some("equal"),
        Some("greater"),
        Some("question"),
        Some("at"),
        Some("A"),
        Some("B"),
        Some("C"),
        Some("D"),
        Some("E"),
        Some("F"),
        Some("G"),
        Some("H"),
        Some("I"),
        Some("J"),
        Some("K"),
        Some("L"),
        Some("M"),
        Some("N"),
        Some("O"),
        Some("P"),
        Some("Q"),
        Some("R"),
        Some("S"),
        Some("T"),
        Some("U"),
        Some("V"),
        Some("W"),
        Some("X"),
        Some("Y"),
        Some("Z"),
        Some("bracketleft"),
        Some("backslash"),
        Some("bracketright"),
        Some("asciicircum"),
        Some("underscore"),
        Some("grave"),
        Some("a"),
        Some("b"),
        Some("c"),
        Some("d"),
        Some("e"),
        Some("f"),
        Some("g"),
        Some("h"),
        Some("i"),
        Some("j"),
        Some("k"),
        Some("l"),
        Some("m"),
        Some("n"),
        Some("o"),
        Some("p"),
        Some("q"),
        Some("r"),
        Some("s"),
        Some("t"),
        Some("u"),
        Some("v"),
        Some("w"),
        Some("x"),
        Some("y"),
        Some("z"),
        Some("braceleft"),
        Some("bar"),
        Some("braceright"),
        Some("asciitilde"),
        Some("bullet"),
        Some("Euro"),
        Some("bullet"),
        Some("quotesinglbase"),
        Some("florin"),
        Some("quotedblbase"),
        Some("ellipsis"),
        Some("dagger"),
        Some("daggerdbl"),
        Some("circumflex"),
        Some("perthousand"),
        Some("Scaron"),
        Some("guilsinglleft"),
        Some("OE"),
        Some("bullet"),
        Some("Zcaron"),
        Some("bullet"),
        Some("bullet"),
        Some("quoteleft"),
        Some("quoteright"),
        Some("quotedblleft"),
        Some("quotedblright"),
        Some("bullet"),
        Some("endash"),
        Some("emdash"),
        Some("tilde"),
        Some("trademark"),
        Some("scaron"),
        Some("guilsinglright"),
        Some("oe"),
        Some("bullet"),
        Some("zcaron"),
        Some("Ydieresis"),
        Some("space"),
        Some("exclamdown"),
        Some("cent"),
        Some("sterling"),
        Some("currency"),
        Some("yen"),
        Some("brokenbar"),
        Some("section"),
        Some("dieresis"),
        Some("copyright"),
        Some("ordfeminine"),
        Some("guillemotleft"),
        Some("logicalnot"),
        Some("hyphen"),
        Some("registered"),
        Some("macron"),
        Some("degree"),
        Some("plusminus"),
        Some("twosuperior"),
        Some("threesuperior"),
        Some("acute"),
        Some("mu"),
        Some("paragraph"),
        Some("periodcentered"),
        Some("cedilla"),
        Some("onesuperior"),
        Some("ordmasculine"),
        Some("guillemotright"),
        Some("onequarter"),
        Some("onehalf"),
        Some("threequarters"),
        Some("questiondown"),
        Some("Agrave"),
        Some("Aacute"),
        Some("Acircumflex"),
        Some("Atilde"),
        Some("Adieresis"),
        Some("Aring"),
        Some("AE"),
        Some("Ccedilla"),
        Some("Egrave"),
        Some("Eacute"),
        Some("Ecircumflex"),
        Some("Edieresis"),
        Some("Igrave"),
        Some("Iacute"),
        Some("Icircumflex"),
        Some("Idieresis"),
        Some("Eth"),
        Some("Ntilde"),
        Some("Ograve"),
        Some("Oacute"),
        Some("Ocircumflex"),
        Some("Otilde"),
        Some("Odieresis"),
        Some("multiply"),
        Some("Oslash"),
        Some("Ugrave"),
        Some("Uacute"),
        Some("Ucircumflex"),
        Some("Udieresis"),
        Some("Yacute"),
        Some("Thorn"),
        Some("germandbls"),
        Some("agrave"),
        Some("aacute"),
        Some("acircumflex"),
        Some("atilde"),
        Some("adieresis"),
        Some("aring"),
        Some("ae"),
        Some("ccedilla"),
        Some("egrave"),
        Some("eacute"),
        Some("ecircumflex"),
        Some("edieresis"),
        Some("igrave"),
        Some("iacute"),
        Some("icircumflex"),
        Some("idieresis"),
        Some("eth"),
        Some("ntilde"),
        Some("ograve"),
        Some("oacute"),
        Some("ocircumflex"),
        Some("otilde"),
        Some("odieresis"),
        Some("divide"),
        Some("oslash"),
        Some("ugrave"),
        Some("uacute"),
        Some("ucircumflex"),
        Some("udieresis"),
        Some("yacute"),
        Some("thorn"),
        Some("ydieresis"),
    ]);

    pub const MAC_ROMAN: Self = Self([
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some("space"),
        Some("exclam"),
        Some("quotedbl"),
        Some("numbersign"),
        Some("dollar"),
        Some("percent"),
        Some("ampersand"),
        Some("quotesingle"),
        Some("parenleft"),
        Some("parenright"),
        Some("asterisk"),
        Some("plus"),
        Some("comma"),
        Some("hyphen"),
        Some("period"),
        Some("slash"),
        Some("zero"),
        Some("one"),
        Some("two"),
        Some("three"),
        Some("four"),
        Some("five"),
        Some("six"),
        Some("seven"),
        Some("eight"),
        Some("nine"),
        Some("colon"),
        Some("semicolon"),
        Some("less"),
        Some("equal"),
        Some("greater"),
        Some("question"),
        Some("at"),
        Some("A"),
        Some("B"),
        Some("C"),
        Some("D"),
        Some("E"),
        Some("F"),
        Some("G"),
        Some("H"),
        Some("I"),
        Some("J"),
        Some("K"),
        Some("L"),
        Some("M"),
        Some("N"),
        Some("O"),
        Some("P"),
        Some("Q"),
        Some("R"),
        Some("S"),
        Some("T"),
        Some("U"),
        Some("V"),
        Some("W"),
        Some("X"),
        Some("Y"),
        Some("Z"),
        Some("bracketleft"),
        Some("backslash"),
        Some("bracketright"),
        Some("asciicircum"),
        Some("underscore"),
        Some("grave"),
        Some("a"),
        Some("b"),
        Some("c"),
        Some("d"),
        Some("e"),
        Some("f"),
        Some("g"),
        Some("h"),
        Some("i"),
        Some("j"),
        Some("k"),
        Some("l"),
        Some("m"),
        Some("n"),
        Some("o"),
        Some("p"),
        Some("q"),
        Some("r"),
        Some("s"),
        Some("t"),
        Some("u"),
        Some("v"),
        Some("w"),
        Some("x"),
        Some("y"),
        Some("z"),
        Some("braceleft"),
        Some("bar"),
        Some("braceright"),
        Some("asciitilde"),
        None,
        Some("Adieresis"),
        Some("Aring"),
        Some("Ccedilla"),
        Some("Eacute"),
        Some("Ntilde"),
        Some("Odieresis"),
        Some("Udieresis"),
        Some("aacute"),
        Some("agrave"),
        Some("acircumflex"),
        Some("adieresis"),
        Some("atilde"),
        Some("aring"),
        Some("ccedilla"),
        Some("eacute"),
        Some("egrave"),
        Some("ecircumflex"),
        Some("edieresis"),
        Some("iacute"),
        Some("igrave"),
        Some("icircumflex"),
        Some("idieresis"),
        Some("ntilde"),
        Some("oacute"),
        Some("ograve"),
        Some("ocircumflex"),
        Some("odieresis"),
        Some("otilde"),
        Some("uacute"),
        Some("ugrave"),
        Some("ucircumflex"),
        Some("udieresis"),
        Some("dagger"),
        Some("degree"),
        Some("cent"),
        Some("sterling"),
        Some("section"),
        Some("bullet"),
        Some("paragraph"),
        Some("germandbls"),
        Some("registered"),
        Some("copyright"),
        Some("trademark"),
        Some("acute"),
        Some("dieresis"),
        Some("notequal"),
        Some("AE"),
        Some("Oslash"),
        Some("infinity"),
        Some("plusminus"),
        Some("lessequal"),
        Some("greaterequal"),
        Some("yen"),
        Some("mu"),
        Some("partialdiff"),
        Some("summation"),
        Some("product"),
        Some("pi"),
        Some("integral"),
        Some("ordfeminine"),
        Some("ordmasculine"),
        Some("Omega"),
        Some("ae"),
        Some("oslash"),
        Some("questiondown"),
        Some("exclamdown"),
        Some("logicalnot"),
        Some("radical"),
        Some("florin"),
        Some("approxequal"),
        Some("Delta"),
        Some("guillemotleft"),
        Some("guillemotright"),
        Some("ellipsis"),
        Some("space"),
        Some("Agrave"),
        Some("Atilde"),
        Some("Otilde"),
        Some("OE"),
        Some("oe"),
        Some("endash"),
        Some("emdash"),
        Some("quotedblleft"),
        Some("quotedblright"),
        Some("quoteleft"),
        Some("quoteright"),
        Some("divide"),
        Some("lozenge"),
        Some("ydieresis"),
        Some("Ydieresis"),
        Some("fraction"),
        Some("currency"),
        Some("guilsinglleft"),
        Some("guilsinglright"),
        Some("fi"),
        Some("fl"),
        Some("daggerdbl"),
        Some("periodcentered"),
        Some("quotesinglbase"),
        Some("quotedblbase"),
        Some("perthousand"),
        Some("Acircumflex"),
        Some("Ecircumflex"),
        Some("Aacute"),
        Some("Edieresis"),
        Some("Egrave"),
        Some("Iacute"),
        Some("Icircumflex"),
        Some("Idieresis"),
        Some("Igrave"),
        Some("Oacute"),
        Some("Ocircumflex"),
        Some("apple"),
        Some("Ograve"),
        Some("Uacute"),
        Some("Ucircumflex"),
        Some("Ugrave"),
        Some("dotlessi"),
        Some("circumflex"),
        Some("tilde"),
        Some("macron"),
        Some("breve"),
        Some("dotaccent"),
        Some("ring"),
        Some("cedilla"),
        Some("hungarumlaut"),
        Some("ogonek"),
        Some("caron"),
    ]);

    pub const MAC_EXPORT: Self = Self([
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some("space"),
        Some("exclamsmall"),
        Some("Hungarumlautsmall"),
        Some("centoldstyle"),
        Some("dollaroldstyle"),
        Some("dollarsuperior"),
        Some("ampersandsmall"),
        Some("Acutesmall"),
        Some("parenleftsuperior"),
        Some("parenrightsuperior"),
        Some("twodotenleader"),
        Some("onedotenleader"),
        Some("comma"),
        Some("hyphen"),
        Some("period"),
        Some("fraction"),
        Some("zerooldstyle"),
        Some("oneoldstyle"),
        Some("twooldstyle"),
        Some("threeoldstyle"),
        Some("fouroldstyle"),
        Some("fiveoldstyle"),
        Some("sixoldstyle"),
        Some("sevenoldstyle"),
        Some("eightoldstyle"),
        Some("nineoldstyle"),
        Some("colon"),
        Some("semicolon"),
        None,
        Some("threequartersemdash"),
        None,
        Some("questionsmall"),
        None,
        None,
        None,
        None,
        Some("Ethsmall"),
        None,
        None,
        Some("onequarter"),
        Some("onehalf"),
        Some("threequarters"),
        Some("oneeighth"),
        Some("threeeighths"),
        Some("fiveeighths"),
        Some("seveneighths"),
        Some("onethird"),
        Some("twothirds"),
        None,
        None,
        None,
        None,
        None,
        None,
        Some("ff"),
        Some("fi"),
        Some("fl"),
        Some("ffi"),
        Some("ffl"),
        Some("parenleftinferior"),
        None,
        Some("parenrightinferior"),
        Some("Circumflexsmall"),
        Some("hypheninferior"),
        Some("Gravesmall"),
        Some("Asmall"),
        Some("Bsmall"),
        Some("Csmall"),
        Some("Dsmall"),
        Some("Esmall"),
        Some("Fsmall"),
        Some("Gsmall"),
        Some("Hsmall"),
        Some("Ismall"),
        Some("Jsmall"),
        Some("Ksmall"),
        Some("Lsmall"),
        Some("Msmall"),
        Some("Nsmall"),
        Some("Osmall"),
        Some("Psmall"),
        Some("Qsmall"),
        Some("Rsmall"),
        Some("Ssmall"),
        Some("Tsmall"),
        Some("Usmall"),
        Some("Vsmall"),
        Some("Wsmall"),
        Some("Xsmall"),
        Some("Ysmall"),
        Some("Zsmall"),
        Some("colonmonetary"),
        Some("onefitted"),
        Some("rupiah"),
        Some("Tildesmall"),
        None,
        None,
        Some("asuperior"),
        Some("centsuperior"),
        None,
        None,
        None,
        None,
        Some("Aacutesmall"),
        Some("Agravesmall"),
        Some("Acircumflexsmall"),
        Some("Adieresissmall"),
        Some("Atildesmall"),
        Some("Aringsmall"),
        Some("Ccedillasmall"),
        Some("Eacutesmall"),
        Some("Egravesmall"),
        Some("Ecircumflexsmall"),
        Some("Edieresissmall"),
        Some("Iacutesmall"),
        Some("Igravesmall"),
        Some("Icircumflexsmall"),
        Some("Idieresissmall"),
        Some("Ntildesmall"),
        Some("Oacutesmall"),
        Some("Ogravesmall"),
        Some("Ocircumflexsmall"),
        Some("Odieresissmall"),
        Some("Otildesmall"),
        Some("Uacutesmall"),
        Some("Ugravesmall"),
        Some("Ucircumflexsmall"),
        Some("Udieresissmall"),
        None,
        Some("eightsuperior"),
        Some("fourinferior"),
        Some("threeinferior"),
        Some("sixinferior"),
        Some("eightinferior"),
        Some("seveninferior"),
        Some("Scaronsmall"),
        None,
        Some("centinferior"),
        Some("twoinferior"),
        None,
        Some("Dieresissmall"),
        None,
        Some("Caronsmall"),
        Some("osuperior"),
        Some("fiveinferior"),
        None,
        Some("commainferior"),
        Some("periodinferior"),
        Some("Yacutesmall"),
        None,
        Some("dollarinferior"),
        None,
        None,
        Some("Thornsmall"),
        None,
        Some("nineinferior"),
        Some("zeroinferior"),
        Some("Zcaronsmall"),
        Some("AEsmall"),
        Some("Oslashsmall"),
        Some("questiondownsmall"),
        Some("oneinferior"),
        Some("Lslashsmall"),
        None,
        None,
        None,
        None,
        None,
        None,
        Some("Cedillasmall"),
        None,
        None,
        None,
        None,
        None,
        Some("OEsmall"),
        Some("figuredash"),
        Some("hyphensuperior"),
        None,
        None,
        None,
        None,
        Some("exclamdownsmall"),
        None,
        Some("Ydieresissmall"),
        None,
        Some("onesuperior"),
        Some("twosuperior"),
        Some("threesuperior"),
        Some("foursuperior"),
        Some("fivesuperior"),
        Some("sixsuperior"),
        Some("sevensuperior"),
        Some("ninesuperior"),
        Some("zerosuperior"),
        None,
        Some("esuperior"),
        Some("rsuperior"),
        Some("tsuperior"),
        None,
        None,
        Some("isuperior"),
        Some("ssuperior"),
        Some("dsuperior"),
        None,
        None,
        None,
        None,
        None,
        Some("lsuperior"),
        Some("Ogoneksmall"),
        Some("Brevesmall"),
        Some("Macronsmall"),
        Some("bsuperior"),
        Some("nsuperior"),
        Some("msuperior"),
        Some("commasuperior"),
        Some("periodsuperior"),
        Some("Dotaccentsmall"),
        Some("Ringsmall"),
        None,
        None,
        None,
        None,
    ]);

    pub const WIN_ANSI: Self = Self([
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some("space"),
        Some("exclam"),
        Some("quotedbl"),
        Some("numbersign"),
        Some("dollar"),
        Some("percent"),
        Some("ampersand"),
        Some("quotesingle"),
        Some("parenleft"),
        Some("parenright"),
        Some("asterisk"),
        Some("plus"),
        Some("comma"),
        Some("hyphen"),
        Some("period"),
        Some("slash"),
        Some("zero"),
        Some("one"),
        Some("two"),
        Some("three"),
        Some("four"),
        Some("five"),
        Some("six"),
        Some("seven"),
        Some("eight"),
        Some("nine"),
        Some("colon"),
        Some("semicolon"),
        Some("less"),
        Some("equal"),
        Some("greater"),
        Some("question"),
        Some("at"),
        Some("A"),
        Some("B"),
        Some("C"),
        Some("D"),
        Some("E"),
        Some("F"),
        Some("G"),
        Some("H"),
        Some("I"),
        Some("J"),
        Some("K"),
        Some("L"),
        Some("M"),
        Some("N"),
        Some("O"),
        Some("P"),
        Some("Q"),
        Some("R"),
        Some("S"),
        Some("T"),
        Some("U"),
        Some("V"),
        Some("W"),
        Some("X"),
        Some("Y"),
        Some("Z"),
        Some("bracketleft"),
        Some("backslash"),
        Some("bracketright"),
        Some("asciicircum"),
        Some("underscore"),
        Some("grave"),
        Some("a"),
        Some("b"),
        Some("c"),
        Some("d"),
        Some("e"),
        Some("f"),
        Some("g"),
        Some("h"),
        Some("i"),
        Some("j"),
        Some("k"),
        Some("l"),
        Some("m"),
        Some("n"),
        Some("o"),
        Some("p"),
        Some("q"),
        Some("r"),
        Some("s"),
        Some("t"),
        Some("u"),
        Some("v"),
        Some("w"),
        Some("x"),
        Some("y"),
        Some("z"),
        Some("braceleft"),
        Some("bar"),
        Some("braceright"),
        Some("asciitilde"),
        Some("bullet"),
        Some("Euro"),
        Some("bullet"),
        Some("quotesinglbase"),
        Some("florin"),
        Some("quotedblbase"),
        Some("ellipsis"),
        Some("dagger"),
        Some("daggerdbl"),
        Some("circumflex"),
        Some("perthousand"),
        Some("Scaron"),
        Some("guilsinglleft"),
        Some("OE"),
        Some("bullet"),
        Some("Zcaron"),
        Some("bullet"),
        Some("bullet"),
        Some("quoteleft"),
        Some("quoteright"),
        Some("quotedblleft"),
        Some("quotedblright"),
        Some("bullet"),
        Some("endash"),
        Some("emdash"),
        Some("tilde"),
        Some("trademark"),
        Some("scaron"),
        Some("guilsinglright"),
        Some("oe"),
        Some("bullet"),
        Some("zcaron"),
        Some("Ydieresis"),
        Some("space"),
        Some("exclamdown"),
        Some("cent"),
        Some("sterling"),
        Some("currency"),
        Some("yen"),
        Some("brokenbar"),
        Some("section"),
        Some("dieresis"),
        Some("copyright"),
        Some("ordfeminine"),
        Some("guillemotleft"),
        Some("logicalnot"),
        Some("hyphen"),
        Some("registered"),
        Some("macron"),
        Some("degree"),
        Some("plusminus"),
        Some("twosuperior"),
        Some("threesuperior"),
        Some("acute"),
        Some("mu"),
        Some("paragraph"),
        Some("periodcentered"),
        Some("cedilla"),
        Some("onesuperior"),
        Some("ordmasculine"),
        Some("guillemotright"),
        Some("onequarter"),
        Some("onehalf"),
        Some("threequarters"),
        Some("questiondown"),
        Some("Agrave"),
        Some("Aacute"),
        Some("Acircumflex"),
        Some("Atilde"),
        Some("Adieresis"),
        Some("Aring"),
        Some("AE"),
        Some("Ccedilla"),
        Some("Egrave"),
        Some("Eacute"),
        Some("Ecircumflex"),
        Some("Edieresis"),
        Some("Igrave"),
        Some("Iacute"),
        Some("Icircumflex"),
        Some("Idieresis"),
        Some("Eth"),
        Some("Ntilde"),
        Some("Ograve"),
        Some("Oacute"),
        Some("Ocircumflex"),
        Some("Otilde"),
        Some("Odieresis"),
        Some("multiply"),
        Some("Oslash"),
        Some("Ugrave"),
        Some("Uacute"),
        Some("Ucircumflex"),
        Some("Udieresis"),
        Some("Yacute"),
        Some("Thorn"),
        Some("germandbls"),
        Some("agrave"),
        Some("aacute"),
        Some("acircumflex"),
        Some("atilde"),
        Some("adieresis"),
        Some("aring"),
        Some("ae"),
        Some("ccedilla"),
        Some("egrave"),
        Some("eacute"),
        Some("ecircumflex"),
        Some("edieresis"),
        Some("igrave"),
        Some("iacute"),
        Some("icircumflex"),
        Some("idieresis"),
        Some("eth"),
        Some("ntilde"),
        Some("ograve"),
        Some("oacute"),
        Some("ocircumflex"),
        Some("otilde"),
        Some("odieresis"),
        Some("divide"),
        Some("oslash"),
        Some("ugrave"),
        Some("uacute"),
        Some("ucircumflex"),
        Some("udieresis"),
        Some("yacute"),
        Some("thorn"),
        Some("ydieresis"),
    ]);
}

#[cfg(test)]
mod tests;
