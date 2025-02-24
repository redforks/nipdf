use crate::{
    ObjectValueError, Result,
    file::{ObjectResolver, page::ResourceDict},
    graphics::{
        NameOrDictByRef, NameOrStream, Operation, Point, parse_operations,
        trans::{GlyphLength, GlyphToTextSpace},
    },
    object::{Dictionary, Object, PdfObject, PdfObjectCore as _, Stream},
    text::{
        CIDFontType, CIDFontWidths, EncodingDict, EncodingDifferences, FontDescriptorDict,
        FontDescriptorFlags, FontDict, FontType, Type0FontDict, Type3FontDict,
    },
};
use either::Either;
use encoding_rs::Encoding as CharEncoding;
use font_kit::{hinting::HintingOptions, loaders::freetype::Font as FontKitFont};
use fontdb::{Database, Family, Query, Source, Weight};
use log::{debug, error, info, warn};
use nipdf_cff_parser::{File as CffFile, Font as CffFont};
use num_traits::ToPrimitive;
use ouroboros::self_referencing;
use pathfinder_geometry::{line_segment::LineSegment2F, vector::Vector2F};
use phf::phf_map;
use prescript::{
    Encoding, NOTDEF, Name,
    cmap::{CMap, CMapRegistry, WriteMode},
    name, sname,
};
use snafu::{OptionExt, ResultExt, ensure_whatever, whatever};
use std::{
    collections::HashMap,
    ops::RangeInclusive,
    rc::Rc,
    sync::{Arc, LazyLock},
};
use ttf_parser::Face as TTFFace;
use winnow::{Parser as _, combinator::terminated, token::rest};

/// FontWidth used in Type1 and TrueType fonts
struct FirstLastFontWidth {
    range: RangeInclusive<u32>,
    widths: Vec<u32>,
    default_width: u32,
}

impl FirstLastFontWidth {
    pub fn from(font: &FontDict<'_, '_>) -> Result<Option<Self>> {
        let widths = font.widths()?;
        let first_char = font.first_char()?;
        let last_char = font.last_char()?;
        if first_char.is_none() || last_char.is_none() {
            return Ok(None);
        }

        let default_width = font.default_width()?;

        let range = first_char.whatever_context::<_, ObjectValueError>("get first_char")?
            ..=last_char.whatever_context::<_, ObjectValueError>("get last_char")?;
        Ok(Some(Self {
            range,
            default_width,
            widths,
        }))
    }

    fn char_width(&self, ch: u32) -> GlyphLength {
        GlyphLength::new(if self.range.contains(&ch) {
            let idx = (ch - self.range.start()) as usize;
            self.widths[idx]
        } else {
            self.default_width
        } as f32)
    }
}

struct FreeTypeFontWidth<'a> {
    font: &'a FontKitFont,
}

impl<'a> FreeTypeFontWidth<'a> {
    fn new(font: &'a FontKitFont) -> Self {
        Self { font }
    }

    pub fn glyph_width(&self, gid: u32) -> Result<u32> {
        self.font
            .advance(gid)
            .whatever_context::<_, ObjectValueError>("get gid advance")?
            .x()
            .to_u32()
            .whatever_context("convert advance to u32")
    }
}

pub trait PathSink {
    fn move_to(&mut self, to: Point);
    fn line_to(&mut self, to: Point);
    fn quad_to(&mut self, ctrl: Point, to: Point);
    fn cubic_to(&mut self, ctrl1: Point, ctrl2: Point, to: Point);
    fn close(&mut self);
}

pub struct PathSinkWrap<'a, P>(&'a mut P);

impl<S: PathSink> font_kit::outline::OutlineSink for PathSinkWrap<'_, S> {
    fn move_to(&mut self, to: Vector2F) {
        self.0.move_to(Point::new(to.x(), to.y()));
    }

    fn line_to(&mut self, to: Vector2F) {
        self.0.line_to(Point::new(to.x(), to.y()));
    }

    fn quadratic_curve_to(&mut self, ctrl: Vector2F, to: Vector2F) {
        self.0
            .quad_to(Point::new(ctrl.x(), ctrl.y()), Point::new(to.x(), to.y()));
    }

    fn cubic_curve_to(&mut self, ctrl: LineSegment2F, to: Vector2F) {
        self.0.cubic_to(
            Point::new(ctrl.from().x(), ctrl.from().y()),
            Point::new(ctrl.to().x(), ctrl.to().y()),
            Point::new(to.x(), to.y()),
        );
    }

    fn close(&mut self) {
        self.0.close();
    }
}

pub trait GlyphRender<P> {
    fn render(&self, gid: u16, sink: &mut P) -> Result<()>;
}

struct TTFGlyphRender<'a> {
    font: &'a FontKitFont,
}

impl<P: PathSink> GlyphRender<P> for TTFGlyphRender<'_> {
    fn render(&self, gid: u16, sink: &mut P) -> Result<()> {
        self.font
            .outline(gid as u32, HintingOptions::None, &mut PathSinkWrap(sink))
            .whatever_context("get glyph outline")
    }
}

pub trait Font<P> {
    fn font_type(&self) -> FontType;
    fn create_op(&self, cmap_registry: &mut CMapRegistry) -> Result<Box<dyn FontOp + '_>>;
    fn create_glyph_render(&self) -> Result<Box<dyn GlyphRender<P> + '_>>;
    fn as_type3(&self) -> Option<&Type3Font<'_, '_>> {
        None
    }
}

struct EncodingParser<'a, 'b, 'c>(&'c FontDict<'a, 'b>);

type EncodingPair<'a> = (Option<Name>, Option<EncodingDifferences<'a>>);
impl EncodingParser<'_, '_, '_> {
    fn by_name(name: &Name) -> Option<Encoding> {
        let r = Encoding::predefined(name);
        if r.is_none() {
            warn!("Unknown encoding: {}", name.as_str());
        }
        r
    }

    fn by_font_name(font_name: &Name) -> Option<Encoding> {
        let encoding_name = standard_14_type1_font_encoding(font_name);
        encoding_name.and_then(|n| Self::by_name(&n))
    }

    fn resolve_by_encoding_or_font_name(
        pair: &Option<EncodingPair<'_>>,
        font_name: &str,
    ) -> Option<Encoding> {
        pair.as_ref()
            .and_then(|p| p.0.as_ref().and_then(Self::by_name))
            .or_else(|| Self::by_font_name(&name(font_name)))
    }

    fn load_from_file(font_name: &str, font_data: &[u8], is_cff: bool) -> Result<Option<Encoding>> {
        if is_cff {
            info!("scan encoding from cff font. ({})", font_name);
            let cff_file: CffFile<'_> = CffFile::open(font_data)
                .whatever_context::<_, ObjectValueError>("Open cff file")?;
            let font: CffFont<'_> = cff_file
                .iter()
                .whatever_context::<_, ObjectValueError>("iter fonts from cff file")?
                .next()
                .whatever_context::<_, ObjectValueError>("no font in cff?")?;
            Ok(Some(
                font.encodings()
                    .whatever_context::<_, ObjectValueError>("parse cff file encodings")?,
            ))
        } else {
            info!("scan encoding from type1 font. ({})", font_name);
            let type1_font = prescript::Font::parse(font_data)
                .whatever_context::<_, ObjectValueError>("parse type1 font encoding")?;
            Ok(type1_font.encoding().cloned())
        }
    }

    fn guess_by_font_name(font_name: &str) -> Option<Encoding> {
        // if font not embed encoding, use known encoding for the two standard symbol fonts
        if let "Symbol" | "ZapfDingbats" = font_name {
            Some(Encoding::SYMBOL)
        } else {
            None
        }
    }

    fn default_encoding(&self) -> Result<Encoding> {
        if let Some(desc) = self.0.font_descriptor()? {
            if desc.flags()?.contains(FontDescriptorFlags::SYMBOLIC) {
                // If the font is symbolic, try to use the encoding from the FontDict
                if let Some(encoding_pair) = self.encoding_pair()? {
                    if let Some(encoding_name) = encoding_pair.0 {
                        if let Some(encoding) = Self::by_name(&encoding_name) {
                            return Ok(encoding);
                        }
                    }
                }
                warn!(
                    "Symbolic font '{}' no encoding in font dict and file, use empty encoding",
                    desc.font_name()?
                );
                return Ok(Encoding::default());
            }
        }

        Ok(Encoding::STANDARD)
    }

    fn apply_encoding_diff(encoding: Encoding, pair: &Option<EncodingPair<'_>>) -> Encoding {
        if let Some((_, Some(diff))) = pair {
            return diff.apply_differences(encoding);
        }
        encoding
    }

    pub fn type1(&self, is_cff: bool, font_data: &[u8]) -> Result<Encoding> {
        let encoding_pair = self.encoding_pair()?;
        let font_name = self
            .0
            .font_name()
            .whatever_context::<_, ObjectValueError>("get type1 font name")?;
        let r = Self::resolve_by_encoding_or_font_name(&encoding_pair, font_name.as_ref())
            .or_else(
                || match Self::load_from_file(font_name.as_ref(), font_data, is_cff) {
                    Ok(encoding) => encoding,
                    Err(e) => {
                        error!("Failed to load encoding from file: {}", e);
                        None
                    }
                },
            )
            .or_else(|| Self::guess_by_font_name(font_name.as_ref()))
            .map_or_else(|| self.default_encoding(), Ok)?;
        Ok(Self::apply_encoding_diff(r, &encoding_pair))
    }

    pub fn type3(&self) -> Result<Encoding> {
        let encoding_pair = self.encoding_pair()?;
        let r = Self::resolve_by_encoding_or_font_name(&encoding_pair, "")
            .map_or_else(|| self.default_encoding(), Ok)?;
        Ok(Self::apply_encoding_diff(r, &encoding_pair))
    }

    fn encoding_pair(&self) -> Result<Option<EncodingPair<'_>>> {
        let encoding = self.0.encoding()?;
        let Some(encoding) = encoding else {
            return Ok(None);
        };

        Ok(Some(match encoding {
            NameOrDictByRef::Name(name) => (Some(name.clone()), None),
            NameOrDictByRef::Dict(d) => {
                let encoding_dict = EncodingDict::new(d, self.0.resolver())
                    .whatever_context::<_, ObjectValueError>("create EncodingDict")?;
                let encoding_name = encoding_dict.base_encoding()?;
                (encoding_name, encoding_dict.differences()?)
            }
        }))
    }

    pub fn ttf(&self) -> Result<Option<Encoding>> {
        let pair = self.encoding_pair()?;
        let Some(pair) = pair else {
            return Ok(None);
        };

        let r = pair.0.as_ref().map_or_else(Encoding::default, |n| {
            Self::by_name(&n.clone()).unwrap_or_else(Encoding::default)
        });
        Ok(Some(Self::apply_encoding_diff(r, &Some(pair))))
    }
}

struct Type1FontOp<'a> {
    font_width: Either<FirstLastFontWidth, FreeTypeFontWidth<'a>>,
    font: &'a FontKitFont,
    encoding: Encoding,
}

impl<'a> Type1FontOp<'a> {
    fn new(
        font_dict: &FontDict<'_, '_>,
        font: &'a FontKitFont,
        is_cff: bool,
        font_data: &'a [u8],
    ) -> Result<Self> {
        let font_width = FirstLastFontWidth::from(font_dict)?
            .map_or_else(|| Either::Right(FreeTypeFontWidth::new(font)), Either::Left);
        let encoding = EncodingParser(font_dict).type1(is_cff, font_data)?;

        Ok(Self {
            font_width,
            font,
            encoding,
        })
    }

    pub fn new_fallback(font: &'a FontKitFont) -> Self {
        Self {
            font_width: Either::Right(FreeTypeFontWidth::new(font)),
            font,
            encoding: Encoding::WIN_ANSI,
        }
    }
}

impl FontOp for Type1FontOp<'_> {
    fn decode_chars<'d>(&'d self, text: &'d [u8]) -> Result<Vec<u32>> {
        Ok(text.iter().map(|v| *v as u32).collect())
    }

    /// Use font.glyph_for_char() if encoding is None or encoding.replace() returns None
    fn char_to_gid(&self, ch: u32) -> Result<u16> {
        let gid_name = self.encoding.get_str(
            ch.try_into()
                .whatever_context::<_, ObjectValueError>("char to u8")?,
        );
        if let Some(r) = self.font.glyph_by_name(gid_name) {
            r.try_into().whatever_context("convert glyph id to u16")
        } else {
            info!("glyph id not found for char: {:?}/{}", ch, gid_name);
            // .notdef gid is always be 0 for type1 font
            Ok(0)
        }
    }

    fn char_advance(&self, gid: u32) -> Result<GlyphLength> {
        self.font_width.as_ref().either(
            |x| {
                let r = x.char_width(gid);
                if self.units_per_em()? != 1000 {
                    Ok(GlyphLength::new(r.0 / 1000.0 * self.units_per_em()? as f32))
                } else {
                    Ok(r)
                }
            },
            |x| {
                Ok(GlyphLength::new(
                    x.glyph_width(self.char_to_gid(gid)? as u32)? as f32,
                ))
            },
        )
    }

    fn units_per_em(&self) -> Result<u16> {
        self.font
            .metrics()
            .units_per_em
            .try_into()
            .whatever_context("convert units_per_em to u16")
    }
}

/// FallbackFont struct, similar to Type1Font but with specific modifications
pub struct FallbackFont {
    font: FontKitFont,
}

impl FallbackFont {
    /// Creates a new FallbackFont instance
    ///
    /// This method loads the builtin Helvetica font and uses WinAnsiEncoding
    pub fn new() -> Result<Self> {
        // Load the builtin Helvetica font data
        let font_data = Arc::new(
            standard_14_type1_font_data("Helvetica")
                .whatever_context::<_, ObjectValueError>("Fallback font (Helvetica) not found")?
                .to_vec(),
        );

        // Create FontKitFont from the font data
        let font = FontKitFont::from_bytes(font_data.clone(), 0)
            .whatever_context::<_, ObjectValueError>("create FontKitFont for fallback")?;

        Ok(Self { font })
    }

    pub fn create_fallback_op(&self) -> Box<dyn FontOp + '_> {
        Box::new(Type1FontOp::new_fallback(&self.font))
    }
}

impl<P: PathSink> Font<P> for FallbackFont {
    fn font_type(&self) -> FontType {
        FontType::Type1
    }

    fn create_op(&self, _cmap_registry: &mut CMapRegistry) -> Result<Box<dyn FontOp + '_>> {
        Ok(Box::new(Type1FontOp::new_fallback(&self.font)))
    }

    fn create_glyph_render(&self) -> Result<Box<dyn GlyphRender<P> + '_>> {
        Ok(Box::new(TTFGlyphRender { font: &self.font }))
    }
}

/// Font implementation using free-type/(font-kit), to handle Type1 fonts
struct Type1Font<'a> {
    font_data: Vec<u8>,
    is_cff: bool,
    font: FontKitFont,
    font_dict: FontDict<'a, 'a>,
}

impl<'a> Type1Font<'a> {
    fn new(is_cff: bool, data: Vec<u8>, font_dict: FontDict<'a, 'a>) -> Result<Self> {
        debug_assert_eq!(data.capacity(), data.len());

        let font = FontKitFont::from_bytes(data.clone().into(), 0)
            .whatever_context::<_, ObjectValueError>("create FontKitFont")?;
        Ok(Self {
            font_data: data,
            is_cff,
            font,
            font_dict,
        })
    }
}

impl<P: PathSink> Font<P> for Type1Font<'_> {
    fn font_type(&self) -> FontType {
        FontType::Type1
    }

    fn create_op(&self, _cmap_registry: &mut CMapRegistry) -> Result<Box<dyn FontOp + '_>> {
        Ok(Box::new(Type1FontOp::new(
            &self.font_dict,
            &self.font,
            self.is_cff,
            self.font_data.as_slice(),
        )?))
    }

    fn create_glyph_render(&self) -> Result<Box<dyn GlyphRender<P> + '_>> {
        Ok(Box::new(TTFGlyphRender { font: &self.font }))
    }
}

struct TTFFontOp<'a> {
    face: &'a FontKitFont,
    units_per_em: u16,
    encoding: Option<Encoding>,
    font_width: Option<FirstLastFontWidth>,
    ttf_font: TTFFace<'a>,
}

impl<'a> TTFFontOp<'a> {
    pub fn new(
        face: &'a FontKitFont,
        encoding: Option<Encoding>,
        font_width: Option<FirstLastFontWidth>,
        ttf_font: TTFFace<'a>,
    ) -> Result<Self> {
        Ok(Self {
            units_per_em: face
                .metrics()
                .units_per_em
                .try_into()
                .whatever_context::<_, ObjectValueError>("failed convert unit_per_em")?,
            face,
            encoding,
            font_width,
            ttf_font,
        })
    }
}

static GLYPH_NAME_TO_UNICODE: phf::Map<&'static str, u32> = include!("glyph_name_to_unicode.rs");

impl FontOp for TTFFontOp<'_> {
    fn decode_chars(&self, s: &[u8]) -> Result<Vec<u32>> {
        Ok(s.iter().map(|v| *v as u32).collect())
    }

    fn char_to_gid(&self, mut ch: u32) -> Result<u16> {
        if let Some(encoding) = self.encoding.as_ref() {
            let glyph_name = encoding.get_str(
                ch.try_into()
                    .whatever_context::<_, ObjectValueError>("Convert ch to u8")?,
            );
            if glyph_name != NOTDEF {
                if let Some(r) = self.face.glyph_by_name(glyph_name) {
                    return r.try_into().whatever_context("failed convert glyph index");
                } else {
                    // If glyph_name not in font CMap, convert to unicode then resolve by unicode
                    // use Adobe Glyph List to convert glyph name to unicode
                    if let Some(unicode) = GLYPH_NAME_TO_UNICODE.get(glyph_name) {
                        ch = *unicode;
                    }
                }
            }
        }

        if let Some(gid) = self.face.glyph_for_char(
            char::from_u32(ch)
                .whatever_context::<_, ObjectValueError>("convert unicode to char")?,
        ) {
            return gid
                .try_into()
                .whatever_context("failed convert glyph index");
        }
        if let Some(r) = {
            let this = &self;
            glyph_index(&this.ttf_font, ch)
        }? {
            return r.try_into().whatever_context("failed convert glyph index");
        }
        warn!("TTF glyph id not found for char: {}", ch);
        Ok(0)
    }

    fn char_advance(&self, ch: u32) -> Result<GlyphLength> {
        if let Some(font_width) = &self.font_width {
            return Ok(font_width.char_width(ch) / 1000.0 * self.units_per_em as f32);
        }
        let gid = self.char_to_gid(ch)?;

        Ok(GlyphLength::new(
            self.face
                .advance(gid as u32)
                .whatever_context::<_, ObjectValueError>("get char advance")?
                .x(),
        ))
    }

    fn units_per_em(&self) -> Result<u16> {
        Ok(self.units_per_em)
    }
}

struct TTFFont<'a, 'b> {
    typ: FontType,
    font_dict: FontDict<'a, 'b>,
    face: FontKitFont,
    data: Arc<Vec<u8>>,
}

impl<'a, 'b> TTFFont<'a, 'b> {
    fn new(typ: FontType, data: Arc<Vec<u8>>, font_dict: FontDict<'a, 'b>) -> Result<Self> {
        debug_assert!(typ == FontType::TrueType || typ == FontType::Type1);
        let face = FontKitFont::from_bytes(data.clone(), 0)
            .whatever_context::<_, ObjectValueError>("parse TTF Font")?;
        Ok(Self {
            typ,
            font_dict,
            face,
            data,
        })
    }
}

impl<P: PathSink> Font<P> for TTFFont<'_, '_> {
    fn font_type(&self) -> FontType {
        self.typ
    }

    fn create_op(&self, _cmap_registry: &mut CMapRegistry) -> Result<Box<dyn FontOp + '_>> {
        let encoding = EncodingParser(&self.font_dict).ttf()?;
        Ok(Box::new(TTFFontOp::new(
            &self.face,
            encoding,
            FirstLastFontWidth::from(&self.font_dict)?,
            TTFFace::parse(&self.data, 0)
                .whatever_context::<_, ObjectValueError>("parse TTF Font")?,
        )?))
    }

    fn create_glyph_render(&self) -> Result<Box<dyn GlyphRender<P> + '_>> {
        Ok(Box::new(TTFGlyphRender { font: &self.face }))
    }
}

static SYSTEM_FONTS: LazyLock<Database> = LazyLock::new(|| {
    let mut db = Database::new();
    db.load_system_fonts();
    // set fallback font that support Cjk, depends on specific environment,
    // TODO: better way to provide default fonts
    db.set_serif_family("Noto Serif CJK SC");
    db.set_sans_serif_family("Noto Sans CJK SC");
    db
});

/// Remove suffix "MT"/"PSMT" from font name. And remove ",Bold", ",BoldItalic", ".BoldOblique",
/// ",Italic", "-BoldItalic", "-Bold", "-Italic", "-BoldOblique", "-Oblique", "-BoldOblique"
fn normalize_true_type_font_name(name: &str) -> String {
    let names = vec![
        "PSMT",
        "MT",
        ",BoldItalic",
        ".BoldOblique",
        ",Bold",
        ",Italic",
        ",Oblique",
        "-BoldItalic",
        "-BoldOblique",
        "-Bold",
        "-Italic",
        "-Oblique",
    ];

    let mut rv = name.to_owned();
    for n in names {
        if rv.ends_with(n) {
            rv.truncate(rv.len() - n.len());
            break;
        }
    }

    rv
}

/// For historic bugs, some pdf file use internal names for the 14 standard fonts
/// @see https://community.adobe.com/t5/acrobat-discussions/timesnewromanpsmt-also-arialmt-and-other-fonts-error-message/td-p/11115292
///
/// This has been an ongoing issue over the years. A few instances have been
/// found to be due to bugs in Acrobat (in reading PDF files) that we have tried
/// to address as quickly as possible with QFE patch releases. Please make sure
/// your copies of Acrobat are indeed updated to the most recent release.
///
/// This function returns the standard 14 font if the font name is an known internal name.
fn normalize_font_name(name: &str) -> &str {
    match name {
        "Arial" | "ArialMT" | "Helvetica" => "Helvetica",
        "Arial,Bold" | "Arial-Bold" | "Arial-BoldMT" | "Helvetica,Bold" | "Helvetica-Bold" => {
            "Helvetica-Bold"
        }
        "Arial,BoldItalic"
        | "Arial-BoldItalic"
        | "Arial-BoldItalicMT"
        | "Helvetica,BoldItalic"
        | "Helvetica-BoldItalic"
        | "Helvetica-BoldOblique" => "Helvetica-BoldOblique",
        "Arial,Italic" | "Arial-Italic" | "Arial-ItalicMT" | "Helvetica,Italic"
        | "Helvetica-Italic" | "Helvetica-Oblique" => "Helvetica-Oblique",
        "Courier" | "CourierNew" | "CourierNewPSMT" => "Courier",
        "Courier,Bold"
        | "Courier-Bold"
        | "CourierNew,Bold"
        | "CourierNew-Bold"
        | "CourierNewPS-BoldMT" => "Courier-Bold",
        "Courier,BoldItalic"
        | "Courier-BoldOblique"
        | "CourierNew,BoldItalic"
        | "CourierNew-BoldItalic"
        | "CourierNewPS-BoldItalicMT" => "Courier-BoldOblique",
        "Courier,Italic"
        | "Courier-Oblique"
        | "CourierNew,Italic"
        | "CourierNew-Italic"
        | "CourierNewPS-ItalicMT" => "Courier-Oblique",
        "Symbol" | "Symbol,Bold" | "Symbol,BoldItalic" | "Symbol,Italic" => "Symbol",
        "Times-Bold"
        | "TimesNewRoman,Bold"
        | "TimesNewRoman-Bold"
        | "TimesNewRomanPS-Bold"
        | "TimesNewRomanPS-BoldMT"
        | "TimesNewRomanPSMT,Bold" => "Times-Bold",
        "Times-BoldItalic"
        | "TimesNewRoman,BoldItalic"
        | "TimesNewRoman-BoldItalic"
        | "TimesNewRomanPS-BoldItalic"
        | "TimesNewRomanPS-BoldItalicMT"
        | "TimesNewRomanPSMT,BoldItalic" => "Times-BoldItalic",
        "Times-Italic"
        | "TimesNewRoman,Italic"
        | "TimesNewRoman-Italic"
        | "TimesNewRomanPS-Italic"
        | "TimesNewRomanPS-ItalicMT"
        | "TimesNewRomanPSMT,Italic" => "Times-Italic",
        "Times-Roman" | "TimesNewRoman" | "TimesNewRomanPS" | "TimesNewRomanPSMT" => "Times-Roman",
        "ZapfDingbats" => "ZapfDingbats",
        others => others,
    }
}

/// If font_name is a standard 14 font, return its Encoding name
fn standard_14_type1_font_encoding(font_name: &str) -> Option<Name> {
    match normalize_font_name(font_name) {
        "Courier"
        | "Courier-Bold"
        | "Courier-BoldOblique"
        | "Courier-Oblique"
        | "Helvetica"
        | "Helvetica-Bold"
        | "Helvetica-BoldOblique"
        | "Helvetica-Oblique"
        | "Times-Bold"
        | "Times-BoldItalic"
        | "Times-Italic"
        | "Times-Roman" => Some(sname("StandardEncoding")),
        "Symbol" => Some(sname("Symbol")),
        "ZapfDingbats" => Some(sname("ZapfDingbats")),
        _ => None,
    }
}

fn standard_14_type1_font_data(font_name: &str) -> Option<&'static [u8]> {
    let font_name = normalize_font_name(font_name);

    match font_name {
        "Courier" => Some(&include_bytes!("../../../../fonts/n022003l.pfb")[..]),
        "Courier-Bold" => Some(&include_bytes!("../../../../fonts/n022004l.pfb")[..]),
        "Courier-BoldOblique" => Some(&include_bytes!("../../../../fonts/n022024l.pfb")[..]),
        "Courier-Oblique" => Some(&include_bytes!("../../../../fonts/n022023l.pfb")[..]),
        "Helvetica" => Some(&include_bytes!("../../../../fonts/n019003l.pfb")[..]),
        "Helvetica-Bold" => Some(&include_bytes!("../../../../fonts/n019004l.pfb")[..]),
        "Helvetica-BoldOblique" => Some(&include_bytes!("../../../../fonts/n019024l.pfb")[..]),
        "Helvetica-Oblique" => Some(&include_bytes!("../../../../fonts/n019023l.pfb")[..]),
        "Symbol" => Some(&include_bytes!("../../../../fonts/s050000l.pfb")[..]),
        "Times-Bold" => Some(&include_bytes!("../../../../fonts/n021004l.pfb")[..]),
        "Times-BoldItalic" => Some(&include_bytes!("../../../../fonts/n021024l.pfb")[..]),
        "Times-Italic" => Some(&include_bytes!("../../../../fonts/n021023l.pfb")[..]),
        "Times-Roman" => Some(&include_bytes!("../../../../fonts/n021003l.pfb")[..]),
        "ZapfDingbats" => Some(&include_bytes!("../../../../fonts/d050000l.pfb")[..]),
        _ => None,
    }
}

#[self_referencing]
struct FontCacheInner<'c, P: PathSink + 'static> {
    fonts: HashMap<Name, Box<dyn Font<P> + 'c>>,
    cmap_registry: CMapRegistry,
    #[borrows(fonts, mut cmap_registry)]
    #[covariant]
    ops: HashMap<Name, Box<dyn FontOp + 'this>>,
    #[borrows(fonts)]
    #[covariant]
    renders: HashMap<Name, Box<dyn GlyphRender<P> + 'this>>,
    fallback_font: FallbackFont,
    #[borrows(fallback_font)]
    #[covariant]
    fallback_op: Box<dyn FontOp + 'this>,
    #[borrows(fallback_font)]
    #[covariant]
    fallback_render: Box<dyn GlyphRender<P> + 'this>,
}

pub struct FontCache<'c, P: PathSink + 'static> {
    cache: FontCacheInner<'c, P>,
}

impl<'c, P: PathSink + 'static> FontCache<'c, P> {
    fn load_true_type_from_os(desc: &FontDescriptorDict<'_, '_>) -> Result<Vec<u8>> {
        let font_name = desc.font_name()?;
        let font_name = normalize_true_type_font_name(&font_name);
        // let font_name = font_name.to_title_case();
        let mut families = vec![Family::Name(font_name.as_ref())];
        let family = desc.font_family()?;
        if let Some(family) = &family {
            if !family.is_empty() {
                families.push(Family::Name(family));
            }
        }
        let flags = desc.flags()?;
        if flags & FontDescriptorFlags::SERIF == FontDescriptorFlags::SERIF {
            families.push(Family::Serif);
        } else if flags & FontDescriptorFlags::FIXED_PITCH == FontDescriptorFlags::FIXED_PITCH {
            families.push(Family::Monospace);
        } else {
            families.push(Family::SansSerif);
        }
        let style = if flags & FontDescriptorFlags::ITALIC == FontDescriptorFlags::ITALIC {
            fontdb::Style::Italic
        } else {
            fontdb::Style::Normal
        };

        let mut q = Query {
            families: &families,
            weight: desc
                .font_weight()?
                .map_or(Ok::<_, ObjectValueError>(Weight::NORMAL), |v| {
                    Ok(Weight(
                        v.try_into()
                            .whatever_context::<_, ObjectValueError>("Convert to fontdb::Weight")?,
                    ))
                })?,
            style,
            ..Default::default()
        };
        if let Some(stretch) = desc.font_stretch()? {
            q.stretch = stretch.into();
        }
        info!("load ttf font from OS, using query: {:?}", &q);

        let id = SYSTEM_FONTS
            .query(&q)
            .whatever_context::<_, ObjectValueError>("font not found in system")?;
        let face = SYSTEM_FONTS
            .face(id)
            .whatever_context::<_, ObjectValueError>("get system fonts")?;
        info!("loaded ttf font: {:?}", &face.source);
        match face.source {
            Source::File(ref path) => Ok(std::fs::read(path)
                .whatever_context::<_, ObjectValueError>("read ttf file from OS")?),
            Source::Binary(ref bytes) | Source::SharedFile(_, ref bytes) => {
                Ok(bytes.as_ref().as_ref().to_owned())
            }
        }
    }

    fn load_embed_font_bytes(resolver: &ObjectResolver<'_>, s: &Stream) -> Result<Vec<u8>> {
        Ok(s.decode(resolver)
            .whatever_context::<_, ObjectValueError>("decode stream")?
            .into_owned())
    }

    fn load_ttf_parser_font<'a, 'b>(
        font_type: FontType,
        font: FontDict<'a, 'b>,
        desc: Option<&FontDescriptorDict<'a, 'b>>,
    ) -> Result<Box<dyn Font<P> + 'b>> {
        let (is_embed, ttf_bytes) = match desc {
            Some(desc) => {
                match desc.font_file2()? {
                    Some(stream) => {
                        // if font is invalid, load from os
                        let bytes = Self::load_embed_font_bytes(desc.resolver(), stream)?;
                        let bytes = Arc::new(bytes);
                        match FontKitFont::analyze_bytes(bytes.clone()) {
                            Result::Ok(_) => (true, bytes),
                            Err(e) => {
                                warn!(
                                    "Failed load embed ttf-font '{}', try load from OS: {}",
                                    desc.font_name()?,
                                    e
                                );
                                (false, Arc::new(Self::load_true_type_from_os(desc)?))
                            }
                        }
                    }
                    None => (false, Arc::new(Self::load_true_type_from_os(desc)?)),
                }
            }
            None => {
                let d: Dictionary = [
                    (sname("FontName"), Object::Name(font.base_font()?)),
                    (sname("Flags"), Object::Integer(0)),
                ]
                .into_iter()
                .collect();
                let desc = FontDescriptorDict::new(&d, font.resolver())?;
                (false, Arc::new(Self::load_true_type_from_os(&desc)?))
            }
        };

        if font_type == FontType::Type0 {
            Ok(Box::new(CIDFontType2Font::new(is_embed, ttf_bytes, font)?))
        } else {
            Ok(Box::new(TTFFont::new(font.subtype()?, ttf_bytes, font)?))
        }
    }

    /// Load Type1 font, only standard 14 fonts supported, these fonts are replaced
    /// by TrueType fonts scanned from current OS. Because Type1 fonts are not
    /// supported by swash, and the only crate support Type1 fonts is `font`, which
    /// I am not familiar with.
    fn load_type1_font<'a>(font: FontDict<'a, 'a>) -> Result<Type1Font<'a>>
    where
        'a: 'c,
    {
        let f = font.type1()?;
        let font_name = font.font_name()?;
        let desc = f.font_descriptor()?;
        let font_data = desc
            .map(|desc| -> Result<_> {
                let r = desc
                    .font_file()
                    .map(|s| s.map(|s| (false, s)))
                    .transpose()
                    .or_else(
                        || desc.font_file3().map(|s| s.map(|s| (true, s))).transpose(), /* if Compact Font Format*/
                    )
                    .transpose();
                r
            })
            .transpose()?
            .flatten();
        let (is_cff, mut bytes) = match font_data {
            Some(s) => (s.0, Self::load_embed_font_bytes(f.resolver(), s.1)?),
            None => (
                false,
                if let Some(font_data) = standard_14_type1_font_data(font_name.as_ref()) {
                    font_data.to_owned()
                } else {
                    whatever!("Standard 14 type1 font not found: {}", font_name)
                },
            ),
        };
        bytes.shrink_to_fit();
        Type1Font::new(is_cff, bytes, font)
    }

    fn scan_font<'a>(font: FontDict<'a, 'a>) -> Result<Option<Box<dyn Font<P> + 'c>>>
    where
        'a: 'c,
    {
        match font.subtype()? {
            FontType::TrueType => {
                let tt = font.truetype()?;
                let desc = tt.font_descriptor()?;
                Ok(Some(Self::load_ttf_parser_font(
                    FontType::TrueType,
                    font,
                    desc.as_ref(),
                )?))
            }

            FontType::Type0 => {
                let type0_font = font.type0()?;
                let descentdant_fonts = type0_font.descendant_fonts()?;
                ensure_whatever!(
                    descentdant_fonts.len() == 1,
                    "Type0 font should have one descendant fonts"
                );
                let descentdant_font = descentdant_fonts
                    .into_iter()
                    .next()
                    .whatever_context::<_, ObjectValueError>("get type0 font desc")?;
                match descentdant_font.subtype()? {
                    CIDFontType::CIDFontType0 => {
                        let desc = descentdant_font
                            .font_descriptor()?
                            .whatever_context::<_, ObjectValueError>("get CIDFontType0 desc")?;
                        let stream = desc
                            .font_file3()
                            .transpose()
                            .or_else(|| {
                                warn!("CIDFontType0 font_file3 is null, try font_file");
                                desc.font_file().transpose()
                            })
                            .whatever_context::<_, ObjectValueError>(
                                "get CIDFontType0 font stream",
                            )??;
                        Ok(Some(Box::new(CIDFontType0Font::new(
                            font,
                            Self::load_embed_font_bytes(descentdant_font.resolver(), stream)?,
                        )?)))
                    }
                    CIDFontType::CIDFontType2 => {
                        let desc = descentdant_font
                            .font_descriptor()?
                            .whatever_context::<_, ObjectValueError>("get CIDFontType2 desc")?;

                        Ok(Some(Self::load_ttf_parser_font(
                            FontType::Type0,
                            font,
                            Some(&desc),
                        )?))
                    }
                }
            }

            FontType::Type1 => Self::load_type1_font(font.clone())
                .map(|v| -> Option<Box<dyn Font<P> + 'c>> { Some(Box::new(v)) })
                .or_else(|err| {
                    info!(
                        "Failed to load type1 font \"{:?}\", try load as truetype",
                        err
                    );
                    let desc = font
                        .font_descriptor()?
                        .whatever_context::<_, ObjectValueError>("get Type1 font desc")?;
                    Ok(Some(Self::load_ttf_parser_font(
                        FontType::Type1,
                        font,
                        Some(&desc),
                    )?))
                }),

            FontType::Type3 => Ok(Some(Box::new(Type3Font::new(font)?))),
            _ => {
                error!("Unsupported font type: {:?}", font.subtype()?);
                Ok(None)
            }
        }
    }

    pub fn new<'a>(resource: &'c ResourceDict<'a, 'a>) -> Result<Self>
    where
        'a: 'c,
    {
        let font_res = resource
            .font()
            .whatever_context::<_, ObjectValueError>("get font resource")?;
        let mut fonts = HashMap::with_capacity(font_res.len());
        for (k, v) in font_res {
            info!("load font: {:?}", k);
            let font = Self::scan_font(v)?;
            if let Some(font) = font {
                fonts.insert(k, font);
            }
        }

        Ok(Self {
            cache: FontCacheInner::try_new(
                fonts,
                CMapRegistry::new(),
                |fonts, cmap_registry| {
                    let mut ops = HashMap::with_capacity(fonts.len());
                    for (k, v) in fonts {
                        debug!("Create {} font_op", k.as_str());
                        ops.insert(k.clone(), v.create_op(cmap_registry)?);
                    }
                    Ok::<_, ObjectValueError>(ops)
                },
                |fonts| {
                    let mut renders = HashMap::with_capacity(fonts.len());
                    for (k, v) in fonts {
                        renders.insert(k.clone(), v.create_glyph_render()?);
                    }
                    Ok(renders)
                },
                FallbackFont::new().unwrap(),
                |fallback_font| Ok(fallback_font.create_fallback_op()),
                |fallback_font| fallback_font.create_glyph_render(),
            )?,
        })
    }

    pub fn get_font(&self, s: &Name) -> &dyn Font<P> {
        self.cache
            .borrow_fonts()
            .get(s)
            .map(AsRef::as_ref)
            .unwrap_or_else(|| self.cache.borrow_fallback_font())
    }

    pub fn get_op(&self, s: &Name) -> &(dyn FontOp) {
        self.cache
            .borrow_ops()
            .get(s)
            .map(AsRef::as_ref)
            .unwrap_or_else(|| self.cache.borrow_fallback_op().as_ref())
    }

    pub fn get_glyph_render(&self, s: &Name) -> &(dyn GlyphRender<P>) {
        self.cache
            .borrow_renders()
            .get(s)
            .map(AsRef::as_ref)
            .unwrap_or_else(|| self.cache.borrow_fallback_render().as_ref())
    }
}

pub trait FontOp {
    /// Decode char codes to chars, possible using some encoding
    fn decode_chars(&self, s: &[u8]) -> Result<Vec<u32>>;
    fn char_to_gid(&self, ch: u32) -> Result<u16>;
    /// Return glyph width or height based on write_mode
    fn char_advance(&self, ch: u32) -> Result<GlyphLength>;
    fn write_mode(&self) -> WriteMode {
        WriteMode::Horizontal
    }
    fn units_per_em(&self) -> Result<u16> {
        Ok(1000)
    }
}

struct CIDFontType0FontOp {
    widths: Option<CIDFontWidths>,
    // width if horizontal, height if vertical
    default_advance: u32,
    encoding: Option<Rc<CMap>>,
    write_mode: WriteMode,
}

impl CIDFontType0FontOp {
    fn new(font: &Type0FontDict<'_, '_>) -> Result<Self> {
        let mut cmap_registry = CMapRegistry::new();
        let encoding = match font.encoding()? {
            NameOrStream::Name(encoding_name) => {
                if encoding_name == "Identity-H" {
                    None
                } else {
                    Some(
                        cmap_registry
                            .get(&name(encoding_name))
                            .whatever_context::<_, ObjectValueError>("Get cmap")?
                            .whatever_context::<_, ObjectValueError>("Get cmap")?,
                    )
                }
            }
            NameOrStream::Stream(s) => {
                let data = s
                    .decode(font.resolver())
                    .whatever_context::<_, ObjectValueError>("decode cmap from stream")?;
                Some(
                    cmap_registry
                        .add_cmap_file(data.as_ref())
                        .whatever_context::<_, ObjectValueError>("add cmap file")?,
                )
            }
        };

        let cid_fonts = font.descendant_fonts()?;
        let cid_font = &cid_fonts[0];
        let widths = cid_font.w()?;
        let write_mode = if !encoding
            .as_ref()
            .is_none_or(|cmap| cmap.w_mode == WriteMode::Horizontal)
        {
            ensure_whatever!(cid_font.w2()?.is_none(), "TODO: support w2");
            ensure_whatever!(cid_font.dw2()?.is_none(), "TODO support dw2");
            WriteMode::Vertical
        } else {
            WriteMode::Horizontal
        };

        Ok(Self {
            widths,
            default_advance: cid_font.dw()?,
            encoding,
            write_mode,
        })
    }
}

impl FontOp for CIDFontType0FontOp {
    /// `s` each two bytes as a char code, big endian. append 0 if len(s) is odd
    fn decode_chars(&self, s: &[u8]) -> Result<Vec<u32>> {
        if let Some(cmap) = &self.encoding {
            // Use the CMap for decoding if available
            Ok(cmap
                .map(s)
                .whatever_context::<_, ObjectValueError>("map code to cid")?
                .into_iter()
                .map(|ch| ch.0 as u32)
                .collect())
        } else {
            // Fallback to Identity-H decoding
            debug_assert!(s.len() % 2 == 0, "{:?}", s);
            let mut rv = Vec::with_capacity(s.len() / 2);
            for i in 0..s.len() / 2 {
                let ch = u16::from_be_bytes([s[i * 2], s[i * 2 + 1]]);
                rv.push(ch as u32);
            }
            Ok(rv)
        }
    }

    fn char_to_gid(&self, ch: u32) -> Result<u16> {
        ch.try_into().whatever_context("convert ch to u16 gid")
    }

    fn char_advance(&self, ch: u32) -> Result<GlyphLength> {
        let char_width = self
            .widths
            .as_ref()
            .map(|w| w.char_width(ch))
            .transpose()
            .whatever_context::<_, ObjectValueError>("get char width")?
            .flatten()
            .unwrap_or(self.default_advance) as f32;
        Ok(GlyphLength::new(char_width))
    }

    fn write_mode(&self) -> WriteMode {
        self.write_mode
    }
}

/// CID -> GID, GID is u16. stored in [u8], each u16 is big endian
struct CIDToGIDMap(Box<[u8]>);

impl CIDToGIDMap {
    pub fn new(data: Vec<u8>) -> Result<Self> {
        ensure_whatever!(data.len() % 2 == 0, "Invalid CIDToGIDMap data length");
        Ok(Self(data.into()))
    }

    pub fn to_gid(&self, ch: usize) -> Option<u16> {
        let idx = ch * 2;
        if idx + 1 >= self.0.len() {
            warn!("(cid_to_gid_map) glyph id not found for char: {}", ch);
            return None;
        }
        Some(u16::from_be_bytes([self.0[idx], self.0[idx + 1]]))
    }
}

struct CIDFontType2FontOp<'a> {
    ttf_face: TTFFace<'a>,
    widths: Option<CIDFontWidths>,
    // width if horizontal, height if vertical
    default_advance: u32,
    units_per_em: u16,
    // Convert as Identity-H if None
    encoding: Option<Rc<CMap>>,
    cid_to_gid: Option<CIDToGIDMap>,
    cid_is_gid: bool,
    write_mode: WriteMode,
}

impl<'a> CIDFontType2FontOp<'a> {
    fn new(
        cmap_registry: &mut CMapRegistry,
        font: Type0FontDict<'_, '_>,
        is_embed: bool,
        ttf_data: &'a [u8],
        widths: Option<CIDFontWidths>,
        default_advance: u32,
        units_per_em: u16,
    ) -> Result<Self> {
        let encoding = match font.encoding()? {
            NameOrStream::Name(encoding_name) => (encoding_name != "Identity-H")
                .then(|| {
                    cmap_registry
                        .get(&name(encoding_name))
                        .whatever_context::<_, ObjectValueError>("Get cmap")?
                        .whatever_context::<_, ObjectValueError>("Get cmap")
                })
                .transpose()?,
            NameOrStream::Stream(s) => {
                ensure_whatever!(
                    font.cmap_stream_dict()?.use_cmap()?.is_none(),
                    "font_dict.use_cmap not supported"
                );
                let data = s
                    .decode(font.resolver())
                    .whatever_context::<_, ObjectValueError>("decode cmap from stream")?;
                Some(
                    cmap_registry
                        .add_cmap_file(data.as_ref())
                        .whatever_context::<_, ObjectValueError>("add cmap file")?,
                )
            }
        };

        let cid_fonts = font.descendant_fonts()?;
        let cid_font = &cid_fonts[0];
        let cid_to_gid = match cid_font.cid_to_gid_map()? {
            NameOrStream::Name(_) => None,
            NameOrStream::Stream(s) => Some(CIDToGIDMap::new(
                s.decode(cid_font.resolver())
                    .whatever_context::<_, ObjectValueError>("decode stream")?
                    .into_owned(),
            )?),
        };
        let write_mode = if !encoding
            .as_ref()
            .is_none_or(|cmap| cmap.w_mode == WriteMode::Horizontal)
        {
            ensure_whatever!(cid_font.w2()?.is_none(), "TODO: support w2");
            ensure_whatever!(cid_font.dw2()?.is_none(), "TODO support dw2");
            WriteMode::Vertical
        } else {
            WriteMode::Horizontal
        };

        let ttf_face = TTFFace::parse(ttf_data, 0)
            .whatever_context::<_, ObjectValueError>("parse TTF Face for CIDFontType2")?;

        Ok(Self {
            ttf_face,
            widths,
            default_advance,
            units_per_em,
            encoding,
            write_mode,
            cid_is_gid: is_embed && cid_to_gid.is_none(),
            cid_to_gid,
        })
    }
}

// TTFFace::glyph_index() ignores non unicode cmap table,
// some non-cjk pdf file use non unicode cmap table. This function
// try to find glyph id from all cmap tables
fn glyph_index(ttf_font: &TTFFace<'_>, ch: u32) -> Result<Option<u16>> {
    for subtable in ttf_font
        .tables()
        .cmap
        .whatever_context::<_, ObjectValueError>("get cmap from TTF Face")?
        .subtables
    {
        if let Some(id) = subtable.glyph_index(ch) {
            return Ok(Some(id.0));
        }
    }

    warn!("glyph id not found from TTF CMap for char: {}", ch);
    Ok(None)
}

impl FontOp for CIDFontType2FontOp<'_> {
    fn decode_chars(&self, s: &[u8]) -> Result<Vec<u32>> {
        self.encoding.as_ref().map_or_else(
            || {
                Ok(s.chunks(2)
                    .map(|ch| ((ch[0] as u32) << 8) | ch[1] as u32)
                    .collect())
            },
            |cmap| {
                Ok(cmap
                    .map(s)
                    .whatever_context::<_, ObjectValueError>("map code to cid")?
                    .into_iter()
                    .map(|ch| ch.0 as u32)
                    .collect())
            },
        )
    }

    fn char_to_gid(&self, ch: u32) -> Result<u16> {
        if self.cid_is_gid {
            return ch.try_into().whatever_context("convert ch to u16 gid");
        }

        self.cid_to_gid.as_ref().map_or_else(
            || {
                glyph_index(&self.ttf_face, ch)?.map_or_else(
                    || ch.try_into().whatever_context("convert ch to u16 gid"),
                    Ok,
                )
            },
            |m| {
                m.to_gid(ch as usize).map_or_else(
                    || {
                        glyph_index(&self.ttf_face, ch)?.map_or_else(
                            || ch.try_into().whatever_context("convert ch to u16 gid"),
                            Ok,
                        )
                    },
                    Ok,
                )
            },
        )
    }

    fn char_advance(&self, ch: u32) -> Result<GlyphLength> {
        let mut char_width = self
            .widths
            .as_ref()
            .map(|w| w.char_width(ch))
            .transpose()
            .whatever_context::<_, ObjectValueError>("get char width")?
            .flatten()
            .unwrap_or(self.default_advance) as f32;
        if self.units_per_em != 1000 {
            char_width = char_width / 1000.0 * self.units_per_em as f32;
        }
        Ok(GlyphLength::new(char_width))
    }

    fn units_per_em(&self) -> Result<u16> {
        Ok(self.units_per_em)
    }

    fn write_mode(&self) -> WriteMode {
        self.write_mode
    }
}

struct CIDFontType2UnicodeFontOp {
    face: FontKitFont,
    width: Option<CIDFontWidths>,
    // width if horizontal, height if vertical
    default_advance: u32,
    units_per_em: u16,
    encoding: &'static CharEncoding,
    write_mode: WriteMode,
}

impl CIDFontType2UnicodeFontOp {
    fn new(
        face: FontKitFont,
        width: Option<CIDFontWidths>,
        default_advance: u32,
        units_per_em: u16,
        encoding: &'static CharEncoding,
        write_mode: WriteMode,
    ) -> Self {
        Self {
            face,
            width,
            default_advance,
            units_per_em,
            encoding,
            write_mode,
        }
    }
}

impl FontOp for CIDFontType2UnicodeFontOp {
    fn decode_chars(&self, s: &[u8]) -> Result<Vec<u32>> {
        let (decoded, _, has_errors) = self.encoding.decode(s);
        if has_errors {
            warn!("Encoding errors occurred while decoding.");
        }
        Ok(decoded.chars().map(|c| c as u32).collect())
    }

    fn char_to_gid(&self, ch: u32) -> Result<u16> {
        let c = char::from_u32(ch)
            .whatever_context::<_, ObjectValueError>("invalid unicode code point")?;

        self.face
            .glyph_for_char(c)
            .map(|glyph| glyph as u16)
            .whatever_context::<_, ObjectValueError>("glyph id not found")
    }

    fn char_advance(&self, ch: u32) -> Result<GlyphLength> {
        let mut char_width = self
            .width
            .as_ref()
            .map(|w| w.char_width(ch))
            .transpose()
            .whatever_context::<_, ObjectValueError>("get char width")?
            .flatten()
            .unwrap_or(self.default_advance) as f32;
        if self.units_per_em != 1000 {
            char_width = char_width / 1000.0 * self.units_per_em as f32;
        }
        Ok(GlyphLength::new(char_width))
    }

    fn units_per_em(&self) -> Result<u16> {
        Ok(self.units_per_em)
    }

    fn write_mode(&self) -> WriteMode {
        self.write_mode
    }
}

/// Font for Type 0 CIDFont, its descendant font is Cff.
struct CIDFontType0Font<'a, 'b> {
    font_dict: FontDict<'a, 'b>,
    font: FontKitFont,
}

impl<'a, 'b> CIDFontType0Font<'a, 'b> {
    fn new(font_dict: FontDict<'a, 'b>, data: Vec<u8>) -> Result<Self> {
        let font = FontKitFont::from_bytes(data.into(), 0)
            .whatever_context::<_, ObjectValueError>("decode FontKitFont for Type0")?;
        Ok(Self { font_dict, font })
    }
}

struct CIDFontType2Font<'a, 'b> {
    data: Arc<Vec<u8>>,
    font: FontKitFont,
    font_dict: FontDict<'a, 'b>,
    font_is_embed: bool,
}

impl<'a, 'b> CIDFontType2Font<'a, 'b> {
    fn new(font_is_embed: bool, data: Arc<Vec<u8>>, font_dict: FontDict<'a, 'b>) -> Result<Self> {
        let font = FontKitFont::from_bytes(data.clone(), 0)
            .whatever_context::<_, ObjectValueError>("decode FontKitFont for Type2")?;
        Ok(Self {
            data,
            font,
            font_dict,
            font_is_embed,
        })
    }
}

impl<P: PathSink + 'static> Font<P> for CIDFontType2Font<'_, '_> {
    fn font_type(&self) -> FontType {
        FontType::Type0
    }

    fn create_op(&self, cmap_registry: &mut CMapRegistry) -> Result<Box<dyn FontOp + '_>> {
        let face = FontKitFont::from_bytes(self.data.clone(), 0)
            .whatever_context::<_, ObjectValueError>("decode FontKitFont for Type2")?;

        // Get the common font info upfront
        let font = self.font_dict.type0()?;
        let encoding = match font.encoding()? {
            NameOrStream::Name(encoding_name) => {
                if encoding_name == "Identity-H" {
                    None
                } else {
                    Some(
                        cmap_registry
                            .get(&name(encoding_name))
                            .whatever_context::<_, ObjectValueError>("Get cmap")?
                            .whatever_context::<_, ObjectValueError>("Get cmap")?,
                    )
                }
            }
            NameOrStream::Stream(s) => {
                let data = s
                    .decode(font.resolver())
                    .whatever_context::<_, ObjectValueError>("decode cmap from stream")?;
                Some(
                    cmap_registry
                        .add_cmap_file(data.as_ref())
                        .whatever_context::<_, ObjectValueError>("add cmap file")?,
                )
            }
        };
        let cid_fonts = font.descendant_fonts()?;
        let cid_font = &cid_fonts[0];
        let widths = cid_font.w()?;
        let default_advance = cid_font.dw()?;
        let units_per_em = face.metrics().units_per_em as u16;
        let write_mode = if !encoding
            .as_ref()
            .is_none_or(|cmap| cmap.w_mode == WriteMode::Horizontal)
        {
            ensure_whatever!(cid_font.w2()?.is_none(), "TODO: support w2");
            ensure_whatever!(cid_font.dw2()?.is_none(), "TODO support dw2");
            WriteMode::Vertical
        } else {
            WriteMode::Horizontal
        };

        // Check encoding and create appropriate FontOp
        if let Some(NameOrDictByRef::Name(ref name)) = self.font_dict.encoding()? {
            if *name != &sname("Identity-H")
                && *name != &sname("Identity-V")
                && Encoding::predefined(name).is_none()
            {
                if *name == &sname("GBK-EUC-H") {
                    return Ok(Box::new(CIDFontType2UnicodeFontOp::new(
                        face,
                        widths,
                        default_advance,
                        units_per_em,
                        encoding_rs::GBK,
                        write_mode,
                    )));
                } else {
                    whatever!("unsupported encoding: '{}'", name)
                }
            }
        }

        Ok(Box::new(CIDFontType2FontOp::new(
            cmap_registry,
            font,
            self.font_is_embed,
            &self.data,
            widths,
            default_advance,
            units_per_em,
        )?))
    }

    fn create_glyph_render(&self) -> Result<Box<dyn GlyphRender<P> + '_>> {
        // Use FreeType, TTFParser failed render bug1734802.pdf
        Ok(Box::new(TTFGlyphRender { font: &self.font }))
    }
}

impl<P: PathSink + 'static> Font<P> for CIDFontType0Font<'_, '_> {
    fn font_type(&self) -> FontType {
        FontType::Type0
    }

    fn create_op(&self, _cmap_registry: &mut CMapRegistry) -> Result<Box<dyn FontOp + '_>> {
        Ok(Box::new(CIDFontType0FontOp::new(&self.font_dict.type0()?)?))
    }

    fn create_glyph_render(&self) -> Result<Box<dyn GlyphRender<P> + '_>> {
        Ok(Box::new(TTFGlyphRender { font: &self.font }))
    }
}

pub struct Type3Glyph(Box<[Operation]>);

impl Type3Glyph {
    pub fn operations(&self) -> &[Operation] {
        &self.0
    }
}

struct Type3FontOp<'a> {
    font_width: FirstLastFontWidth,
    encoding: Encoding,
    name_to_gid: &'a HashMap<Name, u16>,
    units_per_em: u16,
}

impl<'a> Type3FontOp<'a> {
    fn new(font_dict: &FontDict<'_, '_>, name_to_gid: &'a HashMap<Name, u16>) -> Result<Self> {
        let encoding = EncodingParser(font_dict).type3()?;
        let type3 = font_dict.type3()?;
        let matrix = type3.matrix()?;

        Ok(Self {
            font_width: FirstLastFontWidth::from(font_dict)?
                .whatever_context::<_, ObjectValueError>("Get FirstLastFontWidth")?,
            name_to_gid,
            encoding,
            units_per_em: (1.0 / matrix.m11)
                .abs()
                .to_u16()
                .whatever_context::<_, ObjectValueError>("units_per_em to u16")?,
        })
    }
}

impl FontOp for Type3FontOp<'_> {
    fn decode_chars(&self, s: &[u8]) -> Result<Vec<u32>> {
        Ok(s.iter().map(|v| *v as u32).collect())
    }

    fn char_to_gid(&self, ch: u32) -> Result<u16> {
        let gid_name = self.encoding.get_str(
            ch.try_into()
                .whatever_context::<_, ObjectValueError>("convert ch to u8")?,
        );
        if let Some(gid) = self.name_to_gid.get(gid_name) {
            Ok(*gid)
        } else {
            info!("glyph id not found for char: {:?}/{}", ch, gid_name);
            Ok(u16::MAX)
        }
    }

    fn char_advance(&self, ch: u32) -> Result<GlyphLength> {
        Ok(self.font_width.char_width(ch))
    }

    fn units_per_em(&self) -> Result<u16> {
        Ok(self.units_per_em)
    }
}

pub struct Type3Font<'a, 'b> {
    name_to_gid: HashMap<Name, u16>,
    glyphs: Box<[Type3Glyph]>,
    dict: FontDict<'a, 'b>,
}

impl<'a, 'b> Type3Font<'a, 'b> {
    fn parse_glyphs(d: &Type3FontDict<'_, '_>) -> Result<Vec<(Name, Type3Glyph)>> {
        let procs = d.char_procs()?;
        let mut r = Vec::with_capacity(procs.len());
        for (name, stream) in &procs {
            debug!("parse Type3 glyph: {}", name.as_str());
            let data = stream
                .decode(d.resolver())
                .whatever_context::<_, ObjectValueError>("decode stream")?;
            let ops = terminated(parse_operations::<crate::ParserError>, rest)
                .parse(&data[..])
                .map_err(winnow::error::ParseError::into_inner)
                .whatever_context::<_, ObjectValueError>("parse type3 operation")?;
            r.push((name.clone(), Type3Glyph(ops.into())));
        }

        Ok(r)
    }

    pub fn new(dict: FontDict<'a, 'b>) -> Result<Self> {
        let type3 = dict.type3()?;
        let glyph_and_names = Self::parse_glyphs(&type3)?;
        let mut glyphs = Vec::with_capacity(glyph_and_names.len());
        let mut glyph_ids = HashMap::with_capacity(glyph_and_names.len());
        for (name, glyph) in glyph_and_names {
            let gid = glyphs
                .len()
                .try_into()
                .whatever_context::<_, ObjectValueError>("glyphs length convert to u16")?;
            glyphs.push(glyph);
            glyph_ids.insert(name, gid);
        }

        Ok(Self {
            name_to_gid: glyph_ids,
            glyphs: glyphs.into(),
            dict,
        })
    }

    pub fn resources(&self) -> Result<Option<ResourceDict<'_, '_>>> {
        self.dict.type3()?.resources()
    }

    pub fn get_glyph(&self, gid: u16) -> Option<&Type3Glyph> {
        self.glyphs.get(gid as usize)
    }

    pub fn matrix(&self) -> Result<GlyphToTextSpace> {
        self.dict.type3()?.matrix()
    }
}

impl<P: PathSink + 'static> Font<P> for Type3Font<'_, '_> {
    fn font_type(&self) -> FontType {
        FontType::Type3
    }

    fn create_op(&self, _cmap_registry: &mut CMapRegistry) -> Result<Box<dyn FontOp + '_>> {
        Ok(Box::new(Type3FontOp::new(&self.dict, &self.name_to_gid)?))
    }

    fn create_glyph_render(&self) -> Result<Box<dyn GlyphRender<P> + '_>> {
        struct StubGlyphRender;

        impl<P> GlyphRender<P> for StubGlyphRender {
            fn render(&self, _gid: u16, _sink: &mut P) -> Result<()> {
                // Paint::show_texts() do not use GlyphRender to render glyphs
                unreachable!()
            }
        }

        Ok(Box::new(StubGlyphRender))
    }

    fn as_type3(&self) -> Option<&Type3Font<'_, '_>> {
        Some(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    #[test]
    fn first_last_font_width() {
        let font_width = FirstLastFontWidth {
            range: 'a' as u32..='d' as u32,
            widths: vec![100, 200, 300, 400],
            default_width: 15,
        };

        assert_eq!(100.0, font_width.char_width('a' as u32).0);
        assert_eq!(200.0, font_width.char_width('b' as u32).0);
        assert_eq!(400.0, font_width.char_width('d' as u32).0);
        assert_eq!(15.0, font_width.char_width('e' as u32).0);
    }

    #[test_case("s" => "s"; "no need to normalize")]
    #[test_case("TimesNewRomanPSMT" => "TimesNewRoman"; "PSMT")]
    fn test_normalize_true_type_font_name(s: &str) -> String {
        normalize_true_type_font_name(s)
    }
}
