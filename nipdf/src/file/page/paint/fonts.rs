use crate::{
    file::{ObjectResolver, page::ResourceDict},
    graphics::{
        NameOrDictByRef, NameOrStream, Operation, Point, parse_operations,
        trans::{GlyphLength, GlyphToTextSpace},
    },
    object::{PdfObject, Stream},
    text::{
        CIDFontType, CIDFontWidths, EncodingDict, EncodingDifferences, FontDescriptorDict,
        FontDescriptorFlags, FontDict, FontType, Type0FontDict, Type3FontDict,
    },
};
use anyhow::{Ok, Result as AnyResult, anyhow, bail};
use cff_parser::{File as CffFile, Font as CffFont};
use either::Either;
use font_kit::loaders::freetype::Font as FontKitFont;
use fontdb::{Database, Family, Query, Source, Weight};
use heck::ToTitleCase;
use log::{debug, error, info, warn};
use num_traits::ToPrimitive;
use ouroboros::self_referencing;
use pathfinder_geometry::{line_segment::LineSegment2F, vector::Vector2F};
use phf::phf_map;
use prescript::{
    Encoding, NOTDEF, Name,
    cmap::{CMap, CMapRegistry},
    name, sname,
};
use std::{collections::HashMap, ops::RangeInclusive, rc::Rc, sync::LazyLock};
use ttf_parser::{Face as TTFFace, GlyphId, OutlineBuilder};

/// FontWidth used in Type1 and TrueType fonts
struct FirstLastFontWidth {
    range: RangeInclusive<u32>,
    widths: Vec<u32>,
    default_width: u32,
}

impl FirstLastFontWidth {
    pub fn from(font: &FontDict) -> AnyResult<Option<Self>> {
        let widths = font.widths()?;
        let first_char = font.first_char()?;
        let last_char = font.last_char()?;
        if first_char.is_none() || last_char.is_none() {
            return Ok(None);
        }

        let default_width = font.default_width()?;

        let range = first_char.unwrap()..=last_char.unwrap();
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

    pub fn glyph_width(&self, gid: u32) -> u32 {
        self.font.advance(gid).unwrap().x().to_u32().unwrap()
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

impl<'a, S: PathSink> font_kit::outline::OutlineSink for PathSinkWrap<'a, S> {
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

impl<'a, S: PathSink> OutlineBuilder for PathSinkWrap<'a, S> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(Point::new(x, y));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(Point::new(x, y));
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0.quad_to(Point::new(x1, y1), Point::new(x, y));
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0
            .cubic_to(Point::new(x1, y1), Point::new(x2, y2), Point::new(x, y));
    }

    fn close(&mut self) {
        self.0.close();
    }
}
pub trait GlyphRender<P> {
    fn render(&self, gid: u16, sink: &mut P) -> AnyResult<()>;
}

struct Type1GlyphRender<'a> {
    font: &'a FontKitFont,
}

impl<'a, P: PathSink> GlyphRender<P> for Type1GlyphRender<'a> {
    fn render(&self, gid: u16, sink: &mut P) -> AnyResult<()> {
        Ok(self.font.outline(
            gid as u32,
            font_kit::hinting::HintingOptions::None,
            &mut PathSinkWrap(sink),
        )?)
    }
}

pub trait Font<P> {
    fn font_type(&self) -> FontType;
    fn create_op(&self, cmap_registry: &mut CMapRegistry) -> AnyResult<Box<dyn FontOp + '_>>;
    fn create_glyph_render(&self) -> AnyResult<Box<dyn GlyphRender<P> + '_>>;
    fn as_type3(&self) -> Option<&Type3Font> {
        None
    }
}

struct EncodingParser<'a, 'b, 'c>(&'c FontDict<'a, 'b>);

type EncodingPair<'a> = (Option<Name>, Option<EncodingDifferences<'a>>);
impl<'a, 'b, 'c> EncodingParser<'a, 'b, 'c> {
    fn by_name(name: Name) -> Option<Encoding> {
        let r = Encoding::predefined(name.clone());
        if r.is_none() {
            warn!("Unknown encoding: {}", name.as_str());
        }
        r
    }

    fn by_font_name(&self, font_name: &Name) -> Option<Encoding> {
        let encoding_name = standard_14_type1_font_encoding(font_name);
        encoding_name.and_then(Self::by_name)
    }

    fn resolve_by_encoding_or_font_name(
        &self,
        pair: &Option<EncodingPair>,
        font_name: &str,
    ) -> Option<Encoding> {
        pair.as_ref()
            .and_then(|p| p.0.as_ref().and_then(|n| Self::by_name(n.clone())))
            .or_else(|| self.by_font_name(&name(font_name)))
    }

    fn load_from_file(
        font_name: &str,
        font_data: &[u8],
        is_cff: bool,
    ) -> AnyResult<Option<Encoding>> {
        if is_cff {
            info!("scan encoding from cff font. ({})", font_name);
            let cff_file: CffFile = CffFile::open(font_data)?;
            let font: CffFont = cff_file.iter()?.next().expect("no font in cff?");
            Ok(Some(font.encodings()?))
        } else {
            info!("scan encoding from type1 font. ({})", font_name);
            let type1_font = prescript::Font::parse(font_data)?;
            Ok(type1_font.encoding().cloned())
        }
    }

    fn guess_by_font_name(font_name: &str) -> Option<Encoding> {
        // if font not embed encoding, use known encoding for the two standard symbol fonts
        match font_name {
            "Symbol" => Some(Encoding::SYMBOL),
            "ZapfDingbats" => Some(Encoding::SYMBOL),
            _ => None,
        }
    }

    fn default_encoding(&self) -> AnyResult<Encoding> {
        if let Some(desc) = self.0.font_descriptor()? {
            if desc.flags()?.contains(FontDescriptorFlags::SYMBOLIC) {
                panic!("Symbolic font must have encoding, but not found in font file");
            }
        }

        Ok(Encoding::STANDARD)
    }

    fn apply_encoding_diff(&self, encoding: Encoding, pair: &Option<EncodingPair>) -> Encoding {
        if let Some((_, Some(diff))) = pair {
            return diff.apply_differences(encoding);
        }
        encoding
    }

    pub fn type1(&self, is_cff: bool, font_data: &[u8]) -> AnyResult<Encoding> {
        let encoding_pair = self.encoding_pair()?;
        let font_name = self.0.font_name()?;
        let r = self
            .resolve_by_encoding_or_font_name(&encoding_pair, font_name.as_ref())
            .or_else(|| Self::load_from_file(font_name.as_ref(), font_data, is_cff).unwrap())
            .or_else(|| Self::guess_by_font_name(font_name.as_ref()))
            .unwrap_or_else(|| self.default_encoding().unwrap());
        Ok(self.apply_encoding_diff(r, &encoding_pair))
    }

    pub fn type3(&self) -> AnyResult<Encoding> {
        let encoding_pair = self.encoding_pair()?;
        let r = self
            .resolve_by_encoding_or_font_name(&encoding_pair, "")
            .unwrap_or_else(|| self.default_encoding().unwrap());
        Ok(self.apply_encoding_diff(r, &encoding_pair))
    }

    fn encoding_pair(&self) -> AnyResult<Option<EncodingPair>> {
        let encoding = self.0.encoding()?;
        let Some(encoding) = encoding else {
            return Ok(None);
        };

        Ok(Some(match encoding {
            NameOrDictByRef::Name(name) => (Some(name.clone()), None),
            NameOrDictByRef::Dict(d) => {
                let encoding_dict = EncodingDict::new(None, d, self.0.resolver())?;
                let encoding_name = encoding_dict.base_encoding()?;
                (encoding_name, encoding_dict.differences()?)
            }
        }))
    }

    pub fn ttf(&self) -> AnyResult<Option<Encoding>> {
        let pair = self.encoding_pair()?;
        let Some(pair) = pair else {
            return Ok(None);
        };

        let r = pair.0.as_ref().map_or_else(Encoding::default, |n| {
            Self::by_name(n.clone()).unwrap_or_else(Encoding::default)
        });
        Ok(Some(self.apply_encoding_diff(r, &Some(pair))))
    }
}

struct Type1FontOp<'a> {
    font_width: Either<FirstLastFontWidth, FreeTypeFontWidth<'a>>,
    font: &'a FontKitFont,
    encoding: Encoding,
}

impl<'a> Type1FontOp<'a> {
    fn new(
        font_dict: &FontDict,
        font: &'a FontKitFont,
        is_cff: bool,
        font_data: &'a [u8],
    ) -> AnyResult<Self> {
        let font_width = FirstLastFontWidth::from(font_dict)?
            .map_or_else(|| Either::Right(FreeTypeFontWidth::new(font)), Either::Left);
        let encoding = EncodingParser(font_dict).type1(is_cff, font_data)?;

        Ok(Self {
            font_width,
            font,
            encoding,
        })
    }
}

impl<'a> FontOp for Type1FontOp<'a> {
    fn decode_chars<'d>(&'d self, text: &'d [u8]) -> Vec<u32> {
        text.iter().map(|v| *v as u32).collect()
    }

    /// Use font.glyph_for_char() if encoding is None or encoding.replace() returns None
    fn char_to_gid(&self, ch: u32) -> u16 {
        let gid_name = self.encoding.get_str(ch.try_into().unwrap());
        if let Some(r) = self.font.glyph_by_name(gid_name) {
            r.try_into().unwrap()
        } else {
            info!("glyph id not found for char: {:?}/{}", ch, gid_name);
            // .notdef gid is always be 0 for type1 font
            0
        }
    }

    fn char_width(&self, gid: u32) -> GlyphLength {
        self.font_width.as_ref().either(
            |x| {
                let r = x.char_width(gid);
                if self.units_per_em() != 1000 {
                    GlyphLength::new(r.0 / 1000.0 * self.units_per_em() as f32)
                } else {
                    r
                }
            },
            |x| GlyphLength::new(x.glyph_width(self.char_to_gid(gid) as u32) as f32),
        )
    }

    fn units_per_em(&self) -> u16 {
        self.font.metrics().units_per_em.try_into().unwrap()
    }
}

/// Font implementation using free-type/(font-kit), to handle Type1 fonts
struct Type1Font<'a, 'b> {
    font_data: Vec<u8>,
    is_cff: bool,
    font: FontKitFont,
    font_dict: FontDict<'a, 'b>,
}

impl<'a, 'b> Type1Font<'a, 'b> {
    fn new(is_cff: bool, data: Vec<u8>, font_dict: FontDict<'a, 'b>) -> AnyResult<Self> {
        debug_assert_eq!(data.capacity(), data.len());

        let font = FontKitFont::from_bytes(data.clone().into(), 0)?;
        Ok(Self {
            font_data: data,
            is_cff,
            font,
            font_dict,
        })
    }
}

impl<'a, 'b: 'a, P: PathSink> Font<P> for Type1Font<'a, 'b> {
    fn font_type(&self) -> FontType {
        FontType::Type1
    }

    fn create_op(&self, _cmap_registry: &mut CMapRegistry) -> AnyResult<Box<dyn FontOp + '_>> {
        Ok(Box::new(Type1FontOp::new(
            &self.font_dict,
            &self.font,
            self.is_cff,
            self.font_data.as_slice(),
        )?))
    }

    fn create_glyph_render(&self) -> AnyResult<Box<dyn GlyphRender<P> + '_>> {
        Ok(Box::new(Type1GlyphRender { font: &self.font }))
    }
}

struct TTFParserFontOp<'a> {
    face: TTFFace<'a>,
    units_per_em: u16,
    encoding: Option<Encoding>,
    font_width: Option<FirstLastFontWidth>,
}

impl<'a> TTFParserFontOp<'a> {
    pub fn new(
        face: TTFFace<'a>,
        encoding: Option<Encoding>,
        font_width: Option<FirstLastFontWidth>,
    ) -> AnyResult<Self> {
        Ok(Self {
            units_per_em: face.units_per_em(),
            face,
            encoding,
            font_width,
        })
    }
}

static GLYPH_NAME_TO_UNICODE: phf::Map<&'static str, u32> = include!("glyph_name_to_unicode.in");

impl<'a> FontOp for TTFParserFontOp<'a> {
    fn decode_chars(&self, s: &[u8]) -> Vec<u32> {
        s.iter().map(|v| *v as u32).collect()
    }

    fn char_to_gid(&self, ch: u32) -> u16 {
        if let Some(encoding) = self.encoding.as_ref() {
            let glyph_name = encoding.get_str(ch.try_into().unwrap());
            if glyph_name != NOTDEF {
                if let Some(r) = self.face.glyph_index_by_name(glyph_name) {
                    return r.0;
                } else {
                    // If glyph_name not in font CMap, convert to unicode then resolve by unicode
                    // use Adobe Glyph List to convert glyph name to unicode
                    if let Some(unicode) = GLYPH_NAME_TO_UNICODE.get(glyph_name) {
                        if let Some(gid) = self
                            .face
                            .glyph_index(unsafe { char::from_u32_unchecked(*unicode) })
                        {
                            return gid.0;
                        }
                    }
                }
            }
        }

        glyph_index(&self.face, ch).unwrap_or_else(|| {
            warn!("TTF glyph id not found for char: {}", ch);
            0
        })
    }

    fn char_width(&self, ch: u32) -> GlyphLength {
        if let Some(font_width) = &self.font_width {
            return font_width.char_width(ch) / 1000.0 * self.units_per_em as f32;
        }

        GlyphLength::new(
            self.face
                .glyph_hor_advance(GlyphId(self.char_to_gid(ch)))
                .unwrap() as f32,
        )
    }

    fn units_per_em(&self) -> u16 {
        self.units_per_em
    }
}

struct TTFParserGlyphRender<'a> {
    face: TTFFace<'a>,
}

impl<'a, P: PathSink> GlyphRender<P> for TTFParserGlyphRender<'a> {
    fn render(&self, gid: u16, sink: &mut P) -> AnyResult<()> {
        self.face
            .outline_glyph(GlyphId(gid), &mut PathSinkWrap(sink));
        Ok(())
    }
}

struct TTFParserFont<'a, 'b> {
    typ: FontType,
    data: Vec<u8>,
    font_dict: FontDict<'a, 'b>,
}

impl<'a, 'b> TTFParserFont<'a, 'b> {
    fn new(typ: FontType, data: Vec<u8>, font_dict: FontDict<'a, 'b>) -> Self {
        debug_assert!(typ == FontType::TrueType || typ == FontType::Type1);
        Self {
            typ,
            data,
            font_dict,
        }
    }
}

impl<'a, 'b, P: PathSink> Font<P> for TTFParserFont<'a, 'b> {
    fn font_type(&self) -> FontType {
        self.typ
    }

    fn create_op(&self, _cmap_registry: &mut CMapRegistry) -> AnyResult<Box<dyn FontOp + '_>> {
        let face = TTFFace::parse(&self.data, 0)?;
        let encoding = EncodingParser(&self.font_dict).ttf()?;
        Ok(Box::new(TTFParserFontOp::new(
            face,
            encoding,
            FirstLastFontWidth::from(&self.font_dict)?,
        )?))
    }

    fn create_glyph_render(&self) -> AnyResult<Box<dyn GlyphRender<P> + '_>> {
        let face = TTFFace::parse(&self.data, 0)?;
        Ok(Box::new(TTFParserGlyphRender { face }))
    }
}

static SYSTEM_FONTS: LazyLock<Database> = LazyLock::new(|| {
    let mut db = Database::new();
    db.load_system_fonts();
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
        "Arial" => "Helvetica",
        "Arial,Bold" => "Helvetica-Bold",
        "Arial,BoldItalic" => "Helvetica-BoldOblique",
        "Arial,Italic" => "Helvetica-Oblique",
        "Arial-Bold" => "Helvetica-Bold",
        "Arial-BoldItalic" => "Helvetica-BoldOblique",
        "Arial-BoldItalicMT" => "Helvetica-BoldOblique",
        "Arial-BoldMT" => "Helvetica-Bold",
        "Arial-Italic" => "Helvetica-Oblique",
        "Arial-ItalicMT" => "Helvetica-Oblique",
        "ArialMT" => "Helvetica",
        "Courier" => "Courier",
        "Courier,Bold" => "Courier-Bold",
        "Courier,BoldItalic" => "Courier-BoldOblique",
        "Courier,Italic" => "Courier-Oblique",
        "Courier-Bold" => "Courier-Bold",
        "Courier-BoldOblique" => "Courier-BoldOblique",
        "Courier-Oblique" => "Courier-Oblique",
        "CourierNew" => "Courier",
        "CourierNew,Bold" => "Courier-Bold",
        "CourierNew,BoldItalic" => "Courier-BoldOblique",
        "CourierNew,Italic" => "Courier-Oblique",
        "CourierNew-Bold" => "Courier-Bold",
        "CourierNew-BoldItalic" => "Courier-BoldOblique",
        "CourierNew-Italic" => "Courier-Oblique",
        "CourierNewPS-BoldItalicMT" => "Courier-BoldOblique",
        "CourierNewPS-BoldMT" => "Courier-Bold",
        "CourierNewPS-ItalicMT" => "Courier-Oblique",
        "CourierNewPSMT" => "Courier",
        "Helvetica" => "Helvetica",
        "Helvetica,Bold" => "Helvetica-Bold",
        "Helvetica,BoldItalic" => "Helvetica-BoldOblique",
        "Helvetica,Italic" => "Helvetica-Oblique",
        "Helvetica-Bold" => "Helvetica-Bold",
        "Helvetica-BoldItalic" => "Helvetica-BoldOblique",
        "Helvetica-BoldOblique" => "Helvetica-BoldOblique",
        "Helvetica-Italic" => "Helvetica-Oblique",
        "Helvetica-Oblique" => "Helvetica-Oblique",
        "Symbol" => "Symbol",
        "Symbol,Bold" => "Symbol",
        "Symbol,BoldItalic" => "Symbol",
        "Symbol,Italic" => "Symbol",
        "Times-Bold" => "Times-Bold",
        "Times-BoldItalic" => "Times-BoldItalic",
        "Times-Italic" => "Times-Italic",
        "Times-Roman" => "Times-Roman",
        "TimesNewRoman" => "Times-Roman",
        "TimesNewRoman,Bold" => "Times-Bold",
        "TimesNewRoman,BoldItalic" => "Times-BoldItalic",
        "TimesNewRoman,Italic" => "Times-Italic",
        "TimesNewRoman-Bold" => "Times-Bold",
        "TimesNewRoman-BoldItalic" => "Times-BoldItalic",
        "TimesNewRoman-Italic" => "Times-Italic",
        "TimesNewRomanPS" => "Times-Roman",
        "TimesNewRomanPS-Bold" => "Times-Bold",
        "TimesNewRomanPS-BoldItalic" => "Times-BoldItalic",
        "TimesNewRomanPS-BoldItalicMT" => "Times-BoldItalic",
        "TimesNewRomanPS-BoldMT" => "Times-Bold",
        "TimesNewRomanPS-Italic" => "Times-Italic",
        "TimesNewRomanPS-ItalicMT" => "Times-Italic",
        "TimesNewRomanPSMT" => "Times-Roman",
        "TimesNewRomanPSMT,Bold" => "Times-Bold",
        "TimesNewRomanPSMT,BoldItalic" => "Times-BoldItalic",
        "TimesNewRomanPSMT,Italic" => "Times-Italic",
        "ZapfDingbats" => "ZapfDingbats",
        others => others,
    }
}

/// If font_name is a standard 14 font, return its Encoding name
fn standard_14_type1_font_encoding(font_name: &str) -> Option<Name> {
    match normalize_font_name(font_name) {
        "Courier" => Some(sname("StandardEncoding")),
        "Courier-Bold" => Some(sname("StandardEncoding")),
        "Courier-BoldOblique" => Some(sname("StandardEncoding")),
        "Courier-Oblique" => Some(sname("StandardEncoding")),
        "Helvetica" => Some(sname("StandardEncoding")),
        "Helvetica-Bold" => Some(sname("StandardEncoding")),
        "Helvetica-BoldOblique" => Some(sname("StandardEncoding")),
        "Helvetica-Oblique" => Some(sname("StandardEncoding")),
        "Symbol" => Some(sname("Symbol")),
        "Times-Bold" => Some(sname("StandardEncoding")),
        "Times-BoldItalic" => Some(sname("StandardEncoding")),
        "Times-Italic" => Some(sname("StandardEncoding")),
        "Times-Roman" => Some(sname("StandardEncoding")),
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
    #[borrows(fonts)]
    #[covariant]
    ops: HashMap<Name, Box<dyn FontOp + 'this>>,
    #[borrows(fonts)]
    #[covariant]
    renders: HashMap<Name, Box<dyn GlyphRender<P> + 'this>>,
}

pub struct FontCache<'c, P: PathSink + 'static> {
    cache: FontCacheInner<'c, P>,
}

impl<'c, P: PathSink + 'static> FontCache<'c, P> {
    fn load_true_type_from_os(desc: &FontDescriptorDict) -> AnyResult<Vec<u8>> {
        let font_name = desc.font_name()?;
        let font_name = normalize_true_type_font_name(&font_name);
        let font_name = font_name.to_title_case();
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
                .map(|v| Weight(v.try_into().unwrap()))
                .unwrap_or(Weight::NORMAL),
            style,
            ..Default::default()
        };
        if let Some(stretch) = desc.font_stretch()? {
            q.stretch = stretch.into();
        }
        debug!("load ttf font from OS, using query: {:?}", &q);

        let id = SYSTEM_FONTS.query(&q).expect("font not found in system");
        let face = SYSTEM_FONTS.face(id).unwrap();
        debug!("loaded ttf font: {:?}", &face.source);
        assert_eq!(face.index, 0, "Only one face supported");
        match face.source {
            Source::File(ref path) => Ok(std::fs::read(path)?),
            Source::Binary(ref bytes) => Ok(bytes.as_ref().as_ref().to_owned()),
            Source::SharedFile(_, ref bytes) => Ok(bytes.as_ref().as_ref().to_owned()),
        }
    }

    fn load_embed_font_bytes(resolver: &ObjectResolver<'_>, s: &Stream) -> AnyResult<Vec<u8>> {
        Ok(s.decode(resolver)?.into_owned())
    }

    fn load_ttf_parser_font<'a, 'b>(
        font_type: FontType,
        font: FontDict<'a, 'b>,
        desc: FontDescriptorDict<'a, 'b>,
    ) -> AnyResult<Box<dyn Font<P> + 'b>> {
        let (is_embed, ttf_bytes) = match desc.font_file2()? {
            Some(stream) => {
                // if font is invalid, load from os
                let bytes = Self::load_embed_font_bytes(desc.resolver(), stream)?;
                match TTFFace::parse(&bytes, 0) {
                    Result::Ok(_) => (true, bytes),
                    Err(e) => {
                        warn!(
                            "Failed load embed ttf-font '{}', try load from OS: {}",
                            desc.font_name()?,
                            e
                        );
                        (false, Self::load_true_type_from_os(&desc)?)
                    }
                }
            }
            None => (false, Self::load_true_type_from_os(&desc)?),
        };
        if font_type == FontType::Type0 {
            Ok(Box::new(CIDFontType2Font::new(is_embed, ttf_bytes, font)?))
        } else {
            Ok(Box::new(TTFParserFont::new(
                font.subtype()?,
                ttf_bytes,
                font,
            )))
        }
    }

    /// Load Type1 font, only standard 14 fonts supported, these fonts are replaced
    /// by TrueType fonts scanned from current OS. Because Type1 fonts are not
    /// supported by swash, and the only crate support Type1 fonts is `font`, which
    /// I am not familiar with.
    fn load_type1_font<'a, 'b>(font: FontDict<'a, 'b>) -> AnyResult<Type1Font<'a, 'b>>
    where
        'a: 'c,
        'b: 'c,
    {
        let f = font.type1()?;
        let font_name = font.font_name()?;
        let desc = f.font_descriptor()?;
        let font_data = desc
            .map(|desc| -> AnyResult<_> {
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
                    bail!("Standard 14 type1 font not found: {}", font_name)
                },
            ),
        };
        bytes.shrink_to_fit();
        Type1Font::new(is_cff, bytes, font)
    }

    fn scan_font<'a, 'b>(font: FontDict<'a, 'b>) -> AnyResult<Option<Box<dyn Font<P> + 'c>>>
    where
        'a: 'c,
        'b: 'c,
        'b: 'a,
    {
        match font.subtype()? {
            FontType::TrueType => {
                let tt = font.truetype()?;
                let desc = tt.font_descriptor()?.unwrap();
                Ok(Some(Self::load_ttf_parser_font(
                    FontType::TrueType,
                    font,
                    desc,
                )?))
            }

            FontType::Type0 => {
                let type0_font = font.type0()?;
                let descentdant_fonts = type0_font.descendant_fonts()?;
                assert_eq!(
                    descentdant_fonts.len(),
                    1,
                    "Type0 font should have one descendant fonts"
                );
                let descentdant_font = descentdant_fonts.into_iter().next().unwrap();
                match descentdant_font.subtype()? {
                    CIDFontType::CIDFontType0 => {
                        let desc = descentdant_font.font_descriptor()?.unwrap();
                        let stream = desc.font_file3()?.unwrap();
                        Ok(Some(Box::new(CIDFontType0Font::new(
                            font,
                            Self::load_embed_font_bytes(descentdant_font.resolver(), stream)?,
                        )?)))
                    }
                    CIDFontType::CIDFontType2 => {
                        let desc = descentdant_font.font_descriptor()?.unwrap();

                        Ok(Some(Self::load_ttf_parser_font(
                            FontType::Type0,
                            font,
                            desc,
                        )?))
                    }
                }
            }

            FontType::Type1 => Self::load_type1_font(font.clone())
                .map(|v| Some(Box::new(v) as Box<dyn Font<P> + 'c>))
                .or_else(|err| {
                    info!(
                        "Failed to load type1 font \"{:?}\", try load as truetype",
                        err
                    );
                    let desc = font.font_descriptor()?.unwrap();
                    Ok(Some(Self::load_ttf_parser_font(
                        FontType::Type1,
                        font,
                        desc,
                    )?))
                }),

            FontType::Type3 => Ok(Some(Box::new(Type3Font::new(font)?))),
            _ => {
                error!("Unsupported font type: {:?}", font.subtype()?);
                Ok(None)
            }
        }
    }

    pub fn new<'a, 'b>(resource: &'c ResourceDict<'a, 'b>) -> anyhow::Result<Self>
    where
        'a: 'c,
        'b: 'c,
        'b: 'a,
    {
        let font_res = resource.font()?;
        let mut fonts = HashMap::with_capacity(font_res.len());
        for (k, v) in font_res.into_iter() {
            info!("load font: {:?}", k);
            let font = Self::scan_font(v)?;
            if let Some(font) = font {
                fonts.insert(k, font);
            }
        }

        let mut cmap_registry = CMapRegistry::new();
        Ok(Self {
            cache: FontCacheInner::try_new(
                fonts,
                |fonts| {
                    let mut ops = HashMap::with_capacity(fonts.len());
                    for (k, v) in fonts {
                        debug!("Create {} font_op", k.as_str());
                        ops.insert(k.clone(), v.create_op(&mut cmap_registry)?);
                    }
                    Ok(ops)
                },
                |fonts| {
                    let mut renders = HashMap::with_capacity(fonts.len());
                    for (k, v) in fonts {
                        renders.insert(k.clone(), v.create_glyph_render()?);
                    }
                    Ok(renders)
                },
            )?,
        })
    }

    pub fn get_font(&self, s: &Name) -> Option<&dyn Font<P>> {
        self.cache.borrow_fonts().get(s).map(|x| x.as_ref())
    }

    pub fn get_op(&self, s: &Name) -> Option<&(dyn FontOp)> {
        self.cache.borrow_ops().get(s).map(|x| x.as_ref())
    }

    pub fn get_glyph_render(&self, s: &Name) -> Option<&(dyn GlyphRender<P>)> {
        self.cache.borrow_renders().get(s).map(|x| x.as_ref())
    }
}

pub trait FontOp {
    /// Decode char codes to chars, possible using some encoding
    fn decode_chars(&self, s: &[u8]) -> Vec<u32>;
    fn char_to_gid(&self, ch: u32) -> u16;
    /// Return glyph width for specified char
    fn char_width(&self, ch: u32) -> GlyphLength;
    fn units_per_em(&self) -> u16 {
        1000
    }
}

struct CIDFontType0FontOp {
    widths: Option<CIDFontWidths>,
    default_width: u32,
}

impl CIDFontType0FontOp {
    fn new(font: &Type0FontDict) -> AnyResult<Self> {
        if let NameOrStream::Name(encoding) = font.encoding()? {
            assert_eq!(encoding, "Identity-H");
        } else {
            todo!("Only IdentityH encoding supported");
        }
        let cid_fonts = font.descendant_fonts()?;
        let cid_font = &cid_fonts[0];
        let widths = cid_font.w()?;
        Ok(Self {
            widths,
            default_width: cid_font.dw()?,
        })
    }
}

impl FontOp for CIDFontType0FontOp {
    /// `s` each two bytes as a char code, big endian. append 0 if len(s) is odd
    fn decode_chars(&self, s: &[u8]) -> Vec<u32> {
        debug_assert!(s.len() % 2 == 0, "{:?}", s);
        let mut rv = Vec::with_capacity(s.len() / 2);
        for i in 0..s.len() / 2 {
            let ch = u16::from_be_bytes([s[i * 2], s[i * 2 + 1]]);
            rv.push(ch as u32);
        }
        rv
    }

    fn char_to_gid(&self, ch: u32) -> u16 {
        ch.try_into().unwrap()
    }

    fn char_width(&self, ch: u32) -> GlyphLength {
        let char_width = self
            .widths
            .as_ref()
            .and_then(|w| w.char_width(ch))
            .unwrap_or(self.default_width) as f32;
        GlyphLength::new(char_width)
    }
}

/// CID -> GID, GID is u16. stored in [u8], each u16 is big endian
struct CIDToGIDMap(Box<[u8]>);

impl CIDToGIDMap {
    pub fn new(data: Vec<u8>) -> Self {
        assert!(data.len() % 2 == 0);
        Self(data.into())
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
    face: TTFFace<'a>,
    widths: Option<CIDFontWidths>,
    default_width: u32,
    units_per_em: u16,
    // Convert as Identity-{H,V} if None
    cmap: Option<Rc<CMap>>,
    cid_to_gid: Option<CIDToGIDMap>,
    cid_is_gid: bool,
}

impl<'a> CIDFontType2FontOp<'a> {
    fn new(
        cmap_registry: &mut CMapRegistry,
        face: TTFFace<'a>,
        font: &Type0FontDict,
        is_embed: bool,
    ) -> AnyResult<Self> {
        let cmap = match font.encoding()? {
            NameOrStream::Name(encoding_name) => {
                assert!(
                    !(encoding_name.ends_with("-V") || encoding_name == "V"),
                    "todo: Vertical write mode '{}'",
                    encoding_name
                );
                (!(encoding_name == "Identity-H" || encoding_name == "Identity-V"))
                    .then(|| cmap_registry.get(&name(encoding_name)).unwrap())
            }
            NameOrStream::Stream(s) => {
                assert!(
                    font.cmap_stream_dict()?.use_cmap()?.is_none(),
                    "font_dict.use_cmap not supported"
                );
                let data = s.decode(font.resolver())?;
                Some(cmap_registry.add_cmap_file(data.as_ref())?)
            }
        };

        let cid_fonts = font.descendant_fonts()?;
        let cid_font = &cid_fonts[0];
        let cid_to_gid = match cid_font.cid_to_gid_map()? {
            NameOrStream::Name(_) => None,
            NameOrStream::Stream(s) => Some(CIDToGIDMap::new(
                s.decode(cid_font.resolver())?.into_owned(),
            )),
        };
        let widths = cid_font.w()?;

        Ok(Self {
            units_per_em: face.units_per_em(),
            face,
            widths,
            default_width: cid_font.dw()?,
            cmap,
            cid_is_gid: is_embed && cid_to_gid.is_none(),
            cid_to_gid,
        })
    }
}

// TTFFace::glyph_index() ignores non unicode cmap table,
// some non-cjk pdf file use non unicode cmap table. This function
// try to find glyph id from all cmap tables
fn glyph_index(face: &TTFFace, ch: u32) -> Option<u16> {
    for subtable in face.tables().cmap.unwrap().subtables {
        if let Some(id) = subtable.glyph_index(ch) {
            return Some(id.0);
        }
    }

    warn!("glyph id not found from TTF CMap for char: {}", ch);
    None
}

impl<'a> FontOp for CIDFontType2FontOp<'a> {
    fn decode_chars(&self, s: &[u8]) -> Vec<u32> {
        self.cmap.as_ref().map_or_else(
            || {
                s.chunks(2)
                    .map(|ch| (ch[0] as u32) << 8 | ch[1] as u32)
                    .collect()
            },
            |cmap| cmap.map(s).into_iter().map(|ch| ch.0 as u32).collect(),
        )
    }

    fn char_to_gid(&self, ch: u32) -> u16 {
        if self.cid_is_gid {
            return ch.try_into().unwrap();
        }

        self.cid_to_gid.as_ref().map_or_else(
            || {
                glyph_index(&self.face, ch).unwrap_or_else(|| {
                    // warn!("(cid_to_gid_map) glyph id not found for char: {}", ch);
                    ch.try_into().unwrap()
                })
            },
            |m| {
                m.to_gid(ch as usize).unwrap_or_else(|| {
                    glyph_index(&self.face, ch).unwrap_or_else(|| {
                        // warn!("(cid_to_gid_map) glyph id not found for char: {}", ch);
                        ch.try_into().unwrap()
                    })
                })
            },
        )
    }

    fn char_width(&self, ch: u32) -> GlyphLength {
        let mut char_width = self
            .widths
            .as_ref()
            .and_then(|w| w.char_width(ch))
            .unwrap_or(self.default_width) as f32;
        if self.units_per_em != 1000 {
            char_width = char_width / 1000.0 * self.units_per_em as f32;
        }
        GlyphLength::new(char_width)
    }

    fn units_per_em(&self) -> u16 {
        self.units_per_em
    }
}

/// Font for Type 0 CIDFont, its descendant font is Cff.
struct CIDFontType0Font<'a, 'b> {
    font_dict: FontDict<'a, 'b>,
    font: FontKitFont,
}

impl<'a, 'b> CIDFontType0Font<'a, 'b> {
    fn new(font_dict: FontDict<'a, 'b>, data: Vec<u8>) -> AnyResult<Self> {
        let font = FontKitFont::from_bytes(data.into(), 0)?;
        Ok(Self { font_dict, font })
    }
}

struct CIDFontType2Font<'a, 'b> {
    data: Vec<u8>,
    font: FontKitFont,
    font_dict: FontDict<'a, 'b>,
    font_is_embed: bool,
}

impl<'a, 'b> CIDFontType2Font<'a, 'b> {
    fn new(font_is_embed: bool, data: Vec<u8>, font_dict: FontDict<'a, 'b>) -> AnyResult<Self> {
        let font = FontKitFont::from_bytes(data.clone().into(), 0)?;
        Ok(Self {
            data,
            font,
            font_dict,
            font_is_embed,
        })
    }
}

impl<'a, 'b, P: PathSink + 'static> Font<P> for CIDFontType2Font<'a, 'b> {
    fn font_type(&self) -> FontType {
        FontType::Type0
    }

    fn create_op(&self, cmap_registry: &mut CMapRegistry) -> AnyResult<Box<dyn FontOp + '_>> {
        let face = TTFFace::parse(&self.data, 0)?;
        Ok(Box::new(CIDFontType2FontOp::new(
            cmap_registry,
            face,
            &self.font_dict.type0()?,
            self.font_is_embed,
        )?))
    }

    fn create_glyph_render(&self) -> AnyResult<Box<dyn GlyphRender<P> + '_>> {
        // Use FreeType, TTFParser failed render bug1734802.pdf
        Ok(Box::new(Type1GlyphRender { font: &self.font }))
    }
}

impl<'a, 'b, P: PathSink + 'static> Font<P> for CIDFontType0Font<'a, 'b> {
    fn font_type(&self) -> FontType {
        FontType::Type0
    }

    fn create_op(&self, _cmap_registry: &mut CMapRegistry) -> AnyResult<Box<dyn FontOp + '_>> {
        Ok(Box::new(CIDFontType0FontOp::new(&self.font_dict.type0()?)?))
    }

    fn create_glyph_render(&self) -> AnyResult<Box<dyn GlyphRender<P> + '_>> {
        Ok(Box::new(Type1GlyphRender { font: &self.font }))
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
    fn new(font_dict: &FontDict, name_to_gid: &'a HashMap<Name, u16>) -> AnyResult<Self> {
        let encoding = EncodingParser(font_dict).type3()?;
        let type3 = font_dict.type3()?;
        let matrix = type3.matrix()?;

        Ok(Self {
            font_width: FirstLastFontWidth::from(font_dict)?.unwrap(),
            name_to_gid,
            encoding,
            units_per_em: (1.0 / matrix.m11).abs().to_u16().unwrap(),
        })
    }
}

impl<'a> FontOp for Type3FontOp<'a> {
    fn decode_chars(&self, s: &[u8]) -> Vec<u32> {
        s.iter().map(|v| *v as u32).collect()
    }

    fn char_to_gid(&self, ch: u32) -> u16 {
        let gid_name = self.encoding.get_str(ch.try_into().unwrap());
        if let Some(gid) = self.name_to_gid.get(gid_name) {
            *gid
        } else {
            info!("glyph id not found for char: {:?}/{}", ch, gid_name);
            u16::MAX
        }
    }

    fn char_width(&self, ch: u32) -> GlyphLength {
        self.font_width.char_width(ch)
    }

    fn units_per_em(&self) -> u16 {
        self.units_per_em
    }
}

pub struct Type3Font<'a, 'b> {
    name_to_gid: HashMap<Name, u16>,
    glyphs: Box<[Type3Glyph]>,
    dict: FontDict<'a, 'b>,
}

impl<'a, 'b> Type3Font<'a, 'b> {
    fn parse_glyphs(d: &Type3FontDict) -> AnyResult<Vec<(Name, Type3Glyph)>> {
        let procs = d.char_procs()?;
        let mut r = Vec::with_capacity(procs.len());
        for (name, stream) in procs.iter() {
            debug!("parse Type3 glyph: {}", name.as_str());
            let data = stream.decode(d.resolver())?;
            let (_, ops) = parse_operations(&data[..])
                .map_err(|e| anyhow!("parse type3 operation error: {}", e))?;
            r.push((name.clone(), Type3Glyph(ops.into())))
        }

        Ok(r)
    }

    pub fn new(dict: FontDict<'a, 'b>) -> AnyResult<Self> {
        let type3 = dict.type3()?;
        let glyph_and_names = Self::parse_glyphs(&type3)?;
        let mut glyphs = Vec::with_capacity(glyph_and_names.len());
        let mut glyph_ids = HashMap::with_capacity(glyph_and_names.len());
        for (name, glyph) in glyph_and_names {
            let gid = glyphs.len().try_into().unwrap();
            glyphs.push(glyph);
            glyph_ids.insert(name, gid);
        }

        Ok(Self {
            name_to_gid: glyph_ids,
            glyphs: glyphs.into(),
            dict,
        })
    }

    pub fn resources(&self) -> AnyResult<Option<ResourceDict>> {
        self.dict.type3()?.resources()
    }

    pub fn get_glyph(&self, gid: u16) -> Option<&Type3Glyph> {
        self.glyphs.get(gid as usize)
    }

    pub fn matrix(&self) -> AnyResult<GlyphToTextSpace> {
        self.dict.type3()?.matrix()
    }
}

impl<'a, 'b, P: PathSink + 'static> Font<P> for Type3Font<'a, 'b> {
    fn font_type(&self) -> FontType {
        FontType::Type3
    }

    fn create_op(&self, _cmap_registry: &mut CMapRegistry) -> AnyResult<Box<dyn FontOp + '_>> {
        Ok(Box::new(Type3FontOp::new(&self.dict, &self.name_to_gid)?))
    }

    fn create_glyph_render(&self) -> AnyResult<Box<dyn GlyphRender<P> + '_>> {
        struct StubGlyphRender;

        impl<P> GlyphRender<P> for StubGlyphRender {
            fn render(&self, _gid: u16, _sink: &mut P) -> AnyResult<()> {
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
