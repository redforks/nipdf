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

impl<'a, 'b> Type1FontDict<'a, 'b> {
    fn resolve_name(&self) -> anyhow::Result<&str> {
        if let Some(desc) = self.font_descriptor()? {
            return desc.font_name();
        }

        self.base_font()
    }

    pub fn font_name(&self) -> anyhow::Result<&str> {
        let r = self.resolve_name()?;

        // if font is subset, the name will prefixed with a tag,
        // which is a string of 6 uppercase letters, followed by a plus sign (+).
        if r.len() > 7 && r.as_bytes()[6] == b'+' {
            Ok(&r[7..])
        } else {
            Ok(r)
        }
    }
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

    fn font_family(&self) -> String;

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

    fn char_set(&self) -> Option<String>;
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
pub struct Encoding<'a>([&'a str; 256]);

impl<'a> Encoding<'a> {
    pub fn new(encodings: [&'a str; 256]) -> Self {
        Self(encodings)
    }

    pub fn decode(&self, ch: u8) -> &'a str {
        self.0[ch as usize]
    }

    pub fn apply_differences(&self, diff: &EncodingDifferences<'a>) -> Self {
        let mut new = self.0;
        for (ch, name) in diff.0.iter() {
            new[*ch as usize] = *name;
        }
        Self(new)
    }
}

const NOTDEF: &str = ".notdef";

impl Encoding<'static> {
    pub fn predefined(name: &str) -> Option<Self> {
        match name {
            "MacRomanEncoding" => Some(Self::MAC_ROMAN),
            "MacExpertEncoding" => Some(Self::MAC_EXPERT),
            "WinAnsiEncoding" => Some(Self::WIN_ANSI),
            "StandardEncoding" => Some(Self::STANDARD),
            _ => None,
        }
    }

    pub const STANDARD: Self = Self([
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        "space",
        "exclam",
        "quotedbl",
        "numbersign",
        "dollar",
        "percent",
        "ampersand",
        "quotesingle",
        "parenleft",
        "parenright",
        "asterisk",
        "plus",
        "comma",
        "hyphen",
        "period",
        "slash",
        "zero",
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "colon",
        "semicolon",
        "less",
        "equal",
        "greater",
        "question",
        "at",
        "A",
        "B",
        "C",
        "D",
        "E",
        "F",
        "G",
        "H",
        "I",
        "J",
        "K",
        "L",
        "M",
        "N",
        "O",
        "P",
        "Q",
        "R",
        "S",
        "T",
        "U",
        "V",
        "W",
        "X",
        "Y",
        "Z",
        "bracketleft",
        "backslash",
        "bracketright",
        "asciicircum",
        "underscore",
        "grave",
        "a",
        "b",
        "c",
        "d",
        "e",
        "f",
        "g",
        "h",
        "i",
        "j",
        "k",
        "l",
        "m",
        "n",
        "o",
        "p",
        "q",
        "r",
        "s",
        "t",
        "u",
        "v",
        "w",
        "x",
        "y",
        "z",
        "braceleft",
        "bar",
        "braceright",
        "asciitilde",
        "bullet",
        "Euro",
        "bullet",
        "quotesinglbase",
        "florin",
        "quotedblbase",
        "ellipsis",
        "dagger",
        "daggerdbl",
        "circumflex",
        "perthousand",
        "Scaron",
        "guilsinglleft",
        "OE",
        "bullet",
        "Zcaron",
        "bullet",
        "bullet",
        "quoteleft",
        "quoteright",
        "quotedblleft",
        "quotedblright",
        "bullet",
        "endash",
        "emdash",
        "tilde",
        "trademark",
        "scaron",
        "guilsinglright",
        "oe",
        "bullet",
        "zcaron",
        "Ydieresis",
        "space",
        "exclamdown",
        "cent",
        "sterling",
        "currency",
        "yen",
        "brokenbar",
        "section",
        "dieresis",
        "copyright",
        "ordfeminine",
        "guillemotleft",
        "logicalnot",
        "hyphen",
        "registered",
        "macron",
        "degree",
        "plusminus",
        "twosuperior",
        "threesuperior",
        "acute",
        "mu",
        "paragraph",
        "periodcentered",
        "cedilla",
        "onesuperior",
        "ordmasculine",
        "guillemotright",
        "onequarter",
        "onehalf",
        "threequarters",
        "questiondown",
        "Agrave",
        "Aacute",
        "Acircumflex",
        "Atilde",
        "Adieresis",
        "Aring",
        "AE",
        "Ccedilla",
        "Egrave",
        "Eacute",
        "Ecircumflex",
        "Edieresis",
        "Igrave",
        "Iacute",
        "Icircumflex",
        "Idieresis",
        "Eth",
        "Ntilde",
        "Ograve",
        "Oacute",
        "Ocircumflex",
        "Otilde",
        "Odieresis",
        "multiply",
        "Oslash",
        "Ugrave",
        "Uacute",
        "Ucircumflex",
        "Udieresis",
        "Yacute",
        "Thorn",
        "germandbls",
        "agrave",
        "aacute",
        "acircumflex",
        "atilde",
        "adieresis",
        "aring",
        "ae",
        "ccedilla",
        "egrave",
        "eacute",
        "ecircumflex",
        "edieresis",
        "igrave",
        "iacute",
        "icircumflex",
        "idieresis",
        "eth",
        "ntilde",
        "ograve",
        "oacute",
        "ocircumflex",
        "otilde",
        "odieresis",
        "divide",
        "oslash",
        "ugrave",
        "uacute",
        "ucircumflex",
        "udieresis",
        "yacute",
        "thorn",
        "ydieresis",
    ]);

    pub const MAC_ROMAN: Self = Self([
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        "space",
        "exclam",
        "quotedbl",
        "numbersign",
        "dollar",
        "percent",
        "ampersand",
        "quotesingle",
        "parenleft",
        "parenright",
        "asterisk",
        "plus",
        "comma",
        "hyphen",
        "period",
        "slash",
        "zero",
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "colon",
        "semicolon",
        "less",
        "equal",
        "greater",
        "question",
        "at",
        "A",
        "B",
        "C",
        "D",
        "E",
        "F",
        "G",
        "H",
        "I",
        "J",
        "K",
        "L",
        "M",
        "N",
        "O",
        "P",
        "Q",
        "R",
        "S",
        "T",
        "U",
        "V",
        "W",
        "X",
        "Y",
        "Z",
        "bracketleft",
        "backslash",
        "bracketright",
        "asciicircum",
        "underscore",
        "grave",
        "a",
        "b",
        "c",
        "d",
        "e",
        "f",
        "g",
        "h",
        "i",
        "j",
        "k",
        "l",
        "m",
        "n",
        "o",
        "p",
        "q",
        "r",
        "s",
        "t",
        "u",
        "v",
        "w",
        "x",
        "y",
        "z",
        "braceleft",
        "bar",
        "braceright",
        "asciitilde",
        NOTDEF,
        "Adieresis",
        "Aring",
        "Ccedilla",
        "Eacute",
        "Ntilde",
        "Odieresis",
        "Udieresis",
        "aacute",
        "agrave",
        "acircumflex",
        "adieresis",
        "atilde",
        "aring",
        "ccedilla",
        "eacute",
        "egrave",
        "ecircumflex",
        "edieresis",
        "iacute",
        "igrave",
        "icircumflex",
        "idieresis",
        "ntilde",
        "oacute",
        "ograve",
        "ocircumflex",
        "odieresis",
        "otilde",
        "uacute",
        "ugrave",
        "ucircumflex",
        "udieresis",
        "dagger",
        "degree",
        "cent",
        "sterling",
        "section",
        "bullet",
        "paragraph",
        "germandbls",
        "registered",
        "copyright",
        "trademark",
        "acute",
        "dieresis",
        "notequal",
        "AE",
        "Oslash",
        "infinity",
        "plusminus",
        "lessequal",
        "greaterequal",
        "yen",
        "mu",
        "partialdiff",
        "summation",
        "product",
        "pi",
        "integral",
        "ordfeminine",
        "ordmasculine",
        "Omega",
        "ae",
        "oslash",
        "questiondown",
        "exclamdown",
        "logicalnot",
        "radical",
        "florin",
        "approxequal",
        "Delta",
        "guillemotleft",
        "guillemotright",
        "ellipsis",
        "space",
        "Agrave",
        "Atilde",
        "Otilde",
        "OE",
        "oe",
        "endash",
        "emdash",
        "quotedblleft",
        "quotedblright",
        "quoteleft",
        "quoteright",
        "divide",
        "lozenge",
        "ydieresis",
        "Ydieresis",
        "fraction",
        "currency",
        "guilsinglleft",
        "guilsinglright",
        "fi",
        "fl",
        "daggerdbl",
        "periodcentered",
        "quotesinglbase",
        "quotedblbase",
        "perthousand",
        "Acircumflex",
        "Ecircumflex",
        "Aacute",
        "Edieresis",
        "Egrave",
        "Iacute",
        "Icircumflex",
        "Idieresis",
        "Igrave",
        "Oacute",
        "Ocircumflex",
        "apple",
        "Ograve",
        "Uacute",
        "Ucircumflex",
        "Ugrave",
        "dotlessi",
        "circumflex",
        "tilde",
        "macron",
        "breve",
        "dotaccent",
        "ring",
        "cedilla",
        "hungarumlaut",
        "ogonek",
        "caron",
    ]);

    pub const MAC_EXPERT: Self = Self([
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        "space",
        "exclamsmall",
        "Hungarumlautsmall",
        "centoldstyle",
        "dollaroldstyle",
        "dollarsuperior",
        "ampersandsmall",
        "Acutesmall",
        "parenleftsuperior",
        "parenrightsuperior",
        "twodotenleader",
        "onedotenleader",
        "comma",
        "hyphen",
        "period",
        "fraction",
        "zerooldstyle",
        "oneoldstyle",
        "twooldstyle",
        "threeoldstyle",
        "fouroldstyle",
        "fiveoldstyle",
        "sixoldstyle",
        "sevenoldstyle",
        "eightoldstyle",
        "nineoldstyle",
        "colon",
        "semicolon",
        NOTDEF,
        "threequartersemdash",
        NOTDEF,
        "questionsmall",
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        "Ethsmall",
        NOTDEF,
        NOTDEF,
        "onequarter",
        "onehalf",
        "threequarters",
        "oneeighth",
        "threeeighths",
        "fiveeighths",
        "seveneighths",
        "onethird",
        "twothirds",
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        "ff",
        "fi",
        "fl",
        "ffi",
        "ffl",
        "parenleftinferior",
        NOTDEF,
        "parenrightinferior",
        "Circumflexsmall",
        "hypheninferior",
        "Gravesmall",
        "Asmall",
        "Bsmall",
        "Csmall",
        "Dsmall",
        "Esmall",
        "Fsmall",
        "Gsmall",
        "Hsmall",
        "Ismall",
        "Jsmall",
        "Ksmall",
        "Lsmall",
        "Msmall",
        "Nsmall",
        "Osmall",
        "Psmall",
        "Qsmall",
        "Rsmall",
        "Ssmall",
        "Tsmall",
        "Usmall",
        "Vsmall",
        "Wsmall",
        "Xsmall",
        "Ysmall",
        "Zsmall",
        "colonmonetary",
        "onefitted",
        "rupiah",
        "Tildesmall",
        NOTDEF,
        NOTDEF,
        "asuperior",
        "centsuperior",
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        "Aacutesmall",
        "Agravesmall",
        "Acircumflexsmall",
        "Adieresissmall",
        "Atildesmall",
        "Aringsmall",
        "Ccedillasmall",
        "Eacutesmall",
        "Egravesmall",
        "Ecircumflexsmall",
        "Edieresissmall",
        "Iacutesmall",
        "Igravesmall",
        "Icircumflexsmall",
        "Idieresissmall",
        "Ntildesmall",
        "Oacutesmall",
        "Ogravesmall",
        "Ocircumflexsmall",
        "Odieresissmall",
        "Otildesmall",
        "Uacutesmall",
        "Ugravesmall",
        "Ucircumflexsmall",
        "Udieresissmall",
        NOTDEF,
        "eightsuperior",
        "fourinferior",
        "threeinferior",
        "sixinferior",
        "eightinferior",
        "seveninferior",
        "Scaronsmall",
        NOTDEF,
        "centinferior",
        "twoinferior",
        NOTDEF,
        "Dieresissmall",
        NOTDEF,
        "Caronsmall",
        "osuperior",
        "fiveinferior",
        NOTDEF,
        "commainferior",
        "periodinferior",
        "Yacutesmall",
        NOTDEF,
        "dollarinferior",
        NOTDEF,
        NOTDEF,
        "Thornsmall",
        NOTDEF,
        "nineinferior",
        "zeroinferior",
        "Zcaronsmall",
        "AEsmall",
        "Oslashsmall",
        "questiondownsmall",
        "oneinferior",
        "Lslashsmall",
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        "Cedillasmall",
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        "OEsmall",
        "figuredash",
        "hyphensuperior",
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        "exclamdownsmall",
        NOTDEF,
        "Ydieresissmall",
        NOTDEF,
        "onesuperior",
        "twosuperior",
        "threesuperior",
        "foursuperior",
        "fivesuperior",
        "sixsuperior",
        "sevensuperior",
        "ninesuperior",
        "zerosuperior",
        NOTDEF,
        "esuperior",
        "rsuperior",
        "tsuperior",
        NOTDEF,
        NOTDEF,
        "isuperior",
        "ssuperior",
        "dsuperior",
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        "lsuperior",
        "Ogoneksmall",
        "Brevesmall",
        "Macronsmall",
        "bsuperior",
        "nsuperior",
        "msuperior",
        "commasuperior",
        "periodsuperior",
        "Dotaccentsmall",
        "Ringsmall",
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
    ]);

    pub const WIN_ANSI: Self = Self([
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        "space",
        "exclam",
        "quotedbl",
        "numbersign",
        "dollar",
        "percent",
        "ampersand",
        "quotesingle",
        "parenleft",
        "parenright",
        "asterisk",
        "plus",
        "comma",
        "hyphen",
        "period",
        "slash",
        "zero",
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "colon",
        "semicolon",
        "less",
        "equal",
        "greater",
        "question",
        "at",
        "A",
        "B",
        "C",
        "D",
        "E",
        "F",
        "G",
        "H",
        "I",
        "J",
        "K",
        "L",
        "M",
        "N",
        "O",
        "P",
        "Q",
        "R",
        "S",
        "T",
        "U",
        "V",
        "W",
        "X",
        "Y",
        "Z",
        "bracketleft",
        "backslash",
        "bracketright",
        "asciicircum",
        "underscore",
        "grave",
        "a",
        "b",
        "c",
        "d",
        "e",
        "f",
        "g",
        "h",
        "i",
        "j",
        "k",
        "l",
        "m",
        "n",
        "o",
        "p",
        "q",
        "r",
        "s",
        "t",
        "u",
        "v",
        "w",
        "x",
        "y",
        "z",
        "braceleft",
        "bar",
        "braceright",
        "asciitilde",
        "bullet",
        "Euro",
        "bullet",
        "quotesinglbase",
        "florin",
        "quotedblbase",
        "ellipsis",
        "dagger",
        "daggerdbl",
        "circumflex",
        "perthousand",
        "Scaron",
        "guilsinglleft",
        "OE",
        "bullet",
        "Zcaron",
        "bullet",
        "bullet",
        "quoteleft",
        "quoteright",
        "quotedblleft",
        "quotedblright",
        "bullet",
        "endash",
        "emdash",
        "tilde",
        "trademark",
        "scaron",
        "guilsinglright",
        "oe",
        "bullet",
        "zcaron",
        "Ydieresis",
        "space",
        "exclamdown",
        "cent",
        "sterling",
        "currency",
        "yen",
        "brokenbar",
        "section",
        "dieresis",
        "copyright",
        "ordfeminine",
        "guillemotleft",
        "logicalnot",
        "hyphen",
        "registered",
        "macron",
        "degree",
        "plusminus",
        "twosuperior",
        "threesuperior",
        "acute",
        "mu",
        "paragraph",
        "periodcentered",
        "cedilla",
        "onesuperior",
        "ordmasculine",
        "guillemotright",
        "onequarter",
        "onehalf",
        "threequarters",
        "questiondown",
        "Agrave",
        "Aacute",
        "Acircumflex",
        "Atilde",
        "Adieresis",
        "Aring",
        "AE",
        "Ccedilla",
        "Egrave",
        "Eacute",
        "Ecircumflex",
        "Edieresis",
        "Igrave",
        "Iacute",
        "Icircumflex",
        "Idieresis",
        "Eth",
        "Ntilde",
        "Ograve",
        "Oacute",
        "Ocircumflex",
        "Otilde",
        "Odieresis",
        "multiply",
        "Oslash",
        "Ugrave",
        "Uacute",
        "Ucircumflex",
        "Udieresis",
        "Yacute",
        "Thorn",
        "germandbls",
        "agrave",
        "aacute",
        "acircumflex",
        "atilde",
        "adieresis",
        "aring",
        "ae",
        "ccedilla",
        "egrave",
        "eacute",
        "ecircumflex",
        "edieresis",
        "igrave",
        "iacute",
        "icircumflex",
        "idieresis",
        "eth",
        "ntilde",
        "ograve",
        "oacute",
        "ocircumflex",
        "otilde",
        "odieresis",
        "divide",
        "oslash",
        "ugrave",
        "uacute",
        "ucircumflex",
        "udieresis",
        "yacute",
        "thorn",
        "ydieresis",
    ]);

    pub const SYMBOL: Self = Self([
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        "space",
        "exclam",
        "universal",
        "numbersign",
        "existential",
        "percent",
        "ampersand",
        "suchthat",
        "parenleft",
        "parenright",
        "asteriskmath",
        "plus",
        "comma",
        "minus",
        "period",
        "slash",
        "zero",
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "colon",
        "semicolon",
        "less",
        "equal",
        "greater",
        "question",
        "congruent",
        "Alpha",
        "Beta",
        "Chi",
        "Delta",
        "Epsilon",
        "Phi",
        "Gamma",
        "Eta",
        "Iota",
        "theta1",
        "Kappa",
        "Lambda",
        "Mu",
        "Nu",
        "Omicron",
        "Pi",
        "Theta",
        "Rho",
        "Sigma",
        "Tau",
        "Upsilon",
        "sigma1",
        "Omega",
        "Xi",
        "Psi",
        "Zeta",
        "bracketleft",
        "therefore",
        "bracketright",
        "perpendicular",
        "underscore",
        "radicalex",
        "alpha",
        "beta",
        "chi",
        "delta",
        "epsilon",
        "phi",
        "gamma",
        "eta",
        "iota",
        "phi1",
        "kappa",
        "lambda",
        "mu",
        "nu",
        "omicron",
        "pi",
        "theta",
        "rho",
        "sigma",
        "tau",
        "upsilon",
        "omega1",
        "omega",
        "xi",
        "psi",
        "zeta",
        "braceleft",
        "bar",
        "braceright",
        "similar",
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        NOTDEF,
        "Upsilon1",
        "minute",
        "lessequal",
        "fraction",
        "infinity",
        "florin",
        "club",
        "diamond",
        "heart",
        "spade",
        "arrowboth",
        "arrowleft",
        "arrowup",
        "arrowright",
        "arrowdown",
        "degree",
        "plusminus",
        "second",
        "greaterequal",
        "multiply",
        "proportional",
        "partialdiff",
        "bullet",
        "divide",
        "notequal",
        "equivalence",
        "approxequal",
        "ellipsis",
        "arrowvertex",
        "arrowhorizex",
        "carriagereturn",
        "aleph",
        "Ifraktur",
        "Rfraktur",
        "weierstrass",
        "circlemultiply",
        "circleplus",
        "emptyset",
        "intersection",
        "union",
        "propersuperset",
        "reflexsuperset",
        "notsubset",
        "propersubset",
        "reflexsubset",
        "element",
        "notelement",
        "angle",
        "gradient",
        "registerserif",
        "copyrightserif",
        "trademarkserif",
        "product",
        "radical",
        "dotmath",
        "logicalnot",
        "logicaland",
        "logicalor",
        "arrowdblboth",
        "arrowdblleft",
        "arrowdblup",
        "arrowdblright",
        "arrowdbldown",
        "lozenge",
        "angleleft",
        "registersans",
        "copyrightsans",
        "trademarksans",
        "summation",
        "parenlefttp",
        "parenleftex",
        "parenleftbt",
        "bracketlefttp",
        "bracketleftex",
        "bracketleftbt",
        "bracelefttp",
        "braceleftmid",
        "braceleftbt",
        "braceex",
        NOTDEF,
        "angleright",
        "integral",
        "integraltp",
        "integralex",
        "integralbt",
        "parenrighttp",
        "parenrightex",
        "parenrightbt",
        "bracketrighttp",
        "bracketrightex",
        "bracketrightbt",
        "bracerighttp",
        "bracerightmid",
        "bracerightbt",
        NOTDEF,
    ]);

    pub const ZAPFDINGBATS: Self = Self([
        NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF,
        NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF,
        NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, "space",
        "a1", "a2", "a202", "a3", "a4", "a5", "a119", "a118", "a117", "a11", "a12", "a13", "a14",
        "a15", "a16", "a105", "a17", "a18", "a19", "a20", "a21", "a22", "a23", "a24", "a25", "a26",
        "a27", "a28", "a6", "a7", "a8", "a9", "a10", "a29", "a30", "a31", "a32", "a33", "a34",
        "a35", "a36", "a37", "a38", "a39", "a40", "a41", "a42", "a43", "a44", "a45", "a46", "a47",
        "a48", "a49", "a50", "a51", "a52", "a53", "a54", "a55", "a56", "a57", "a58", "a59", "a60",
        "a61", "a62", "a63", "a64", "a65", "a66", "a67", "a68", "a69", "a70", "a71", "a72", "a73",
        "a74", "a203", "a75", "a204", "a76", "a77", "a78", "a79", "a81", "a82", "a83", "a84",
        "a97", "a98", "a99", "a100", NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF,
        NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF,
        NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF,
        NOTDEF, NOTDEF, NOTDEF, NOTDEF, NOTDEF, "a101", "a102", "a103", "a104", "a106", "a107",
        "a108", "a112", "a111", "a110", "a109", "a120", "a121", "a122", "a123", "a124", "a125",
        "a126", "a127", "a128", "a129", "a130", "a131", "a132", "a133", "a134", "a135", "a136",
        "a137", "a138", "a139", "a140", "a141", "a142", "a143", "a144", "a145", "a146", "a147",
        "a148", "a149", "a150", "a151", "a152", "a153", "a154", "a155", "a156", "a157", "a158",
        "a159", "a160", "a161", "a163", "a164", "a196", "a165", "a192", "a166", "a167", "a168",
        "a169", "a170", "a171", "a172", "a173", "a162", "a174", "a175", "a176", "a177", "a178",
        "a179", "a193", "a180", "a199", "a181", "a200", "a182", NOTDEF, "a201", "a183", "a184",
        "a197", "a185", "a194", "a198", "a186", "a195", "a187", "a188", "a189", "a190", "a191",
        NOTDEF,
    ]);
}

#[cfg(test)]
mod tests;
