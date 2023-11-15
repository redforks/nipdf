use crate::{
    file::{page::ResourceDict, ObjectResolver},
    graphics::{NameOrDictByRef, NameOrStream},
    object::{PdfObject, Stream},
    text::{
        CIDFontType, CIDFontWidths, Encoding256, EncodingDict, FontDescriptorDict,
        FontDescriptorFlags, FontDict, FontType, Type0FontDict, Type1FontDict,
    },
};
use anyhow::{anyhow, bail, Ok, Result as AnyResult};
use cff_parser::{File as CffFile, Font as CffFont};
use either::Either;
use font_kit::loaders::freetype::Font as FontKitFont;
use fontdb::{Database, Family, Query, Source, Weight};
use log::{error, info, warn};
use once_cell::sync::Lazy;
use ouroboros::self_referencing;
use pathfinder_geometry::{line_segment::LineSegment2F, vector::Vector2F};
use smallvec::SmallVec;
use std::{borrow::Cow, collections::HashMap, ops::RangeInclusive};
use tiny_skia::PathBuilder;
use ttf_parser::{Face as TTFFace, GlyphId, OutlineBuilder};

/// FontWidth used in Type1 and TrueType fonts
struct FirstLastFontWidth {
    range: RangeInclusive<u32>,
    widths: Vec<u32>,
    default_width: u32,
}

impl FirstLastFontWidth {
    fn _new(first_char: u32, last_char: u32, default_width: u32, widths: Vec<u32>) -> Self {
        let range = first_char..=last_char;

        Self {
            range,
            widths,
            default_width,
        }
    }

    pub fn from_type1_type(font: &Type1FontDict) -> AnyResult<Option<Self>> {
        let widths = font.widths()?;
        let first_char = font.first_char()?;
        let last_char = font.last_char()?;
        if first_char.is_none() || last_char.is_none() {
            return Ok(None);
        }

        let desc = font
            .font_descriptor()?
            .expect("missing font descriptor, if widths exist, descriptor must also exist");
        let default_width = desc.missing_width()?;

        Ok(Some(Self::_new(
            first_char.unwrap(),
            last_char.unwrap(),
            default_width,
            widths,
        )))
    }

    fn char_width(&self, ch: u32) -> u32 {
        if self.range.contains(&ch) {
            let idx = (ch - self.range.start()) as usize;
            self.widths[idx]
        } else {
            self.default_width
        }
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
        self.font.advance(gid).unwrap().x() as u32
    }
}

pub struct PathSink<'a>(pub &'a mut PathBuilder);

struct FreeTypePathSink<'a> {
    path: &'a mut PathBuilder,
    scale: f32,
}

impl<'a> FreeTypePathSink<'a> {
    fn new(path: &'a mut PathBuilder, font_size: f32) -> Self {
        Self {
            path,
            scale: font_size / 1000.0,
        }
    }
}

impl<'a> font_kit::outline::OutlineSink for FreeTypePathSink<'a> {
    fn move_to(&mut self, to: Vector2F) {
        self.path.move_to(to.x() * self.scale, to.y() * self.scale);
    }

    fn line_to(&mut self, to: Vector2F) {
        self.path.line_to(to.x() * self.scale, to.y() * self.scale);
    }

    fn quadratic_curve_to(&mut self, ctrl: Vector2F, to: Vector2F) {
        self.path.quad_to(
            ctrl.x() * self.scale,
            ctrl.y() * self.scale,
            to.x() * self.scale,
            to.y() * self.scale,
        );
    }

    fn cubic_curve_to(&mut self, ctrl: LineSegment2F, to: Vector2F) {
        self.path.cubic_to(
            ctrl.from().x() * self.scale,
            ctrl.from().y() * self.scale,
            ctrl.to().x() * self.scale,
            ctrl.to().y() * self.scale,
            to.x() * self.scale,
            to.y() * self.scale,
        );
    }

    fn close(&mut self) {
        self.path.close();
    }
}

pub trait GlyphRender {
    fn render(&self, gid: u16, font_size: f32, sink: &mut PathSink) -> AnyResult<()>;
}

struct Type1GlyphRender<'a> {
    font: &'a FontKitFont,
}

impl<'a> GlyphRender for Type1GlyphRender<'a> {
    fn render(&self, gid: u16, font_size: f32, sink: &mut PathSink) -> AnyResult<()> {
        let mut sink = FreeTypePathSink::new(sink.0, font_size);
        Ok(self.font.outline(
            gid as u32,
            font_kit::hinting::HintingOptions::None,
            &mut sink,
        )?)
    }
}

pub trait Font {
    fn font_type(&self) -> FontType;
    fn create_op(&self) -> AnyResult<Box<dyn FontOp + '_>>;
    fn create_glyph_render(&self) -> AnyResult<Box<dyn GlyphRender + '_>>;
}

struct Type1FontOp<'a> {
    font_width: Either<FirstLastFontWidth, FreeTypeFontWidth<'a>>,
    font: &'a FontKitFont,
    encoding: Encoding256<'a>,
}

impl<'c> Type1FontOp<'c> {
    fn new<'a: 'c, 'b: 'c>(
        font_dict: Type1FontDict<'a, 'b>,
        font: &'c FontKitFont,
        is_cff: bool,
        font_data: &'c [u8],
    ) -> AnyResult<Self> {
        let font_name = font_dict.font_name()?;
        let resolve_by_name = |encoding_name: Option<&str>| -> AnyResult<Encoding256> {
            if let Some(encoding_name) = encoding_name {
                return Encoding256::predefined(encoding_name)
                    .ok_or_else(|| anyhow!("Unknown encoding: {}", encoding_name));
            }

            if is_cff {
                info!("scan encoding from cff font. ({})", font_name);
                let cff_file: CffFile<'c> = CffFile::open(font_data)?;
                let font: CffFont<'c> = cff_file.iter()?.next().expect("no font in cff?");
                return Ok(Encoding256::borrowed(font.encodings()?));
            } else {
                info!("scan encoding from type1 font. ({})", font_name);
                let type1_font = type1_parser::Font::parse(font_data)?;
                if let Some(encoding) = type1_font.encoding() {
                    let mut encoding256: [String; 256] =
                        std::array::from_fn(|_| ".notdef".to_owned());
                    for (i, name) in encoding.0.iter().enumerate() {
                        if let Some(n) = name {
                            encoding256[i] = n.to_owned();
                        }
                    }
                    return Ok(Encoding256::owned(encoding256));
                }
            }

            // if font not embed encoding, use known encoding for the two standard symbol fonts
            match font_name {
                "Symbol" => {
                    return Ok(Encoding256::SYMBOL);
                }
                "ZapfDingbats" => {
                    return Ok(Encoding256::ZAPFDINGBATS);
                }
                _ => (),
            }

            if let Some(desc) = font_dict.font_descriptor()? {
                if desc.flags()?.contains(FontDescriptorFlags::SYMBOLIC) {
                    panic!("Symbolic font must have encoding, but not found in font file");
                }
            }

            Ok(Encoding256::STANDARD)
        };

        let font_width = FirstLastFontWidth::from_type1_type(&font_dict)?
            .map_or_else(|| Either::Right(FreeTypeFontWidth::new(font)), Either::Left);
        let encoding = font_dict.encoding()?;
        let encoding = match encoding {
            Some(NameOrDictByRef::Dict(d)) => {
                let encoding_dict = EncodingDict::new(None, d, font_dict.resolver())?;
                let r = resolve_by_name(
                    encoding_dict
                        .base_encoding()?
                        .or_else(|| standard_14_type1_font_encoding(font_name)),
                )?;
                if let Some(diff) = encoding_dict.differences()? {
                    r.apply_differences(&diff)
                } else {
                    r
                }
            }
            Some(NameOrDictByRef::Name(name)) => resolve_by_name(Some(name.as_ref()))?,
            None => resolve_by_name(standard_14_type1_font_encoding(font_name))?,
        };
        Ok(Self {
            font_width,
            font,
            encoding,
        })
    }
}

impl<'a> FontOp for Type1FontOp<'a> {
    fn decode_chars<'b>(&'b self, text: &'b [u8]) -> Vec<u32> {
        text.iter().map(|v| *v as u32).collect()
    }

    /// Use font.glyph_for_char() if encoding is None or encoding.replace() returns None
    fn char_to_gid(&self, ch: u32) -> u16 {
        let gid_name = self.encoding.decode(ch as u8);
        if let Some(r) = self.font.glyph_by_name(gid_name) {
            r as u16
        } else {
            info!("glyph id not found for char: {:?}/{}", ch, gid_name);
            // .notdef gid is always be 0 for type1 font
            0
        }
    }

    fn char_width(&self, gid: u32) -> u32 {
        self.font_width.as_ref().either(
            |x| x.char_width(gid),
            |x| x.glyph_width(self.char_to_gid(gid) as u32),
        )
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

impl<'a, 'b> Font for Type1Font<'a, 'b> {
    fn font_type(&self) -> FontType {
        FontType::Type1
    }

    fn create_op(&self) -> AnyResult<Box<dyn FontOp + '_>> {
        Ok(Box::new(Type1FontOp::new(
            self.font_dict.type1()?,
            &self.font,
            self.is_cff,
            self.font_data.as_slice(),
        )?))
    }

    fn create_glyph_render(&self) -> AnyResult<Box<dyn GlyphRender + '_>> {
        Ok(Box::new(Type1GlyphRender { font: &self.font }))
    }
}

struct TTFParserFontOp<'a> {
    face: TTFFace<'a>,
    units_per_em: u16,
}

impl<'a> FontOp for TTFParserFontOp<'a> {
    fn decode_chars(&self, s: &[u8]) -> Vec<u32> {
        s.iter().map(|v| *v as u32).collect()
    }

    fn char_to_gid(&self, ch: u32) -> u16 {
        self.face
            .glyph_index(unsafe { char::from_u32_unchecked(ch) })
            .unwrap_or_else(|| {
                error!("Failed convert char {} to gid", ch);
                GlyphId(ch as u16)
            })
            .0
    }

    fn char_width(&self, ch: u32) -> u32 {
        self.face
            .glyph_hor_advance(GlyphId(self.char_to_gid(ch)))
            .unwrap() as u32
    }

    fn units_per_em(&self) -> u16 {
        self.units_per_em
    }
}

struct TTFParserPathSink<'a> {
    path: &'a mut PathBuilder,
    scale: f32,
}

impl<'a> TTFParserPathSink<'a> {
    pub fn new(path: &'a mut PathBuilder, font_size: f32, units_per_em: f32) -> Self {
        Self {
            path,
            scale: font_size / units_per_em,
        }
    }
}

impl<'a> OutlineBuilder for TTFParserPathSink<'a> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.path.move_to(x * self.scale, y * self.scale);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to(x * self.scale, y * self.scale);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.path.quad_to(
            x1 * self.scale,
            y1 * self.scale,
            x * self.scale,
            y * self.scale,
        );
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.path.cubic_to(
            x1 * self.scale,
            y1 * self.scale,
            x2 * self.scale,
            y2 * self.scale,
            x * self.scale,
            y * self.scale,
        );
    }

    fn close(&mut self) {
        self.path.close()
    }
}

struct TTFParserGlyphRender<'a> {
    face: TTFFace<'a>,
    units_per_em: f32,
}

impl<'a> GlyphRender for TTFParserGlyphRender<'a> {
    fn render(&self, gid: u16, font_size: f32, sink: &mut PathSink) -> AnyResult<()> {
        let mut sink = TTFParserPathSink::new(sink.0, font_size, self.units_per_em);
        self.face.outline_glyph(GlyphId(gid), &mut sink);
        Ok(())
    }
}

struct TTFParserFont<'a, 'b> {
    typ: FontType,
    data: Vec<u8>,
    font_dict: FontDict<'a, 'b>,
}

impl<'a, 'b> Font for TTFParserFont<'a, 'b> {
    fn font_type(&self) -> FontType {
        self.typ
    }

    fn create_op(&self) -> AnyResult<Box<dyn FontOp + '_>> {
        Ok(match self.font_type() {
            FontType::TrueType | FontType::Type1 => {
                let face = TTFFace::parse(&self.data, 0)?;
                Box::new(TTFParserFontOp {
                    units_per_em: face.units_per_em(),
                    face,
                })
            }
            FontType::Type0 => Box::new(Type0FontOp::new(&self.font_dict.type0()?)?),
            _ => unreachable!(
                "TTFParserFont not support font type: {:?}",
                self.font_type()
            ),
        })
    }

    fn create_glyph_render(&self) -> AnyResult<Box<dyn GlyphRender + '_>> {
        let face = TTFFace::parse(&self.data, 0)?;
        Ok(Box::new(TTFParserGlyphRender {
            units_per_em: face.units_per_em() as f32,
            face,
        }))
    }
}

static SYSTEM_FONTS: Lazy<Database> = Lazy::new(|| {
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
fn standard_14_type1_font_encoding(font_name: &str) -> Option<&'static str> {
    match normalize_font_name(font_name) {
        "Courier" => Some("StandardEncoding"),
        "Courier-Bold" => Some("StandardEncoding"),
        "Courier-BoldOblique" => Some("StandardEncoding"),
        "Courier-Oblique" => Some("StandardEncoding"),
        "Helvetica" => Some("StandardEncoding"),
        "Helvetica-Bold" => Some("StandardEncoding"),
        "Helvetica-BoldOblique" => Some("StandardEncoding"),
        "Helvetica-Oblique" => Some("StandardEncoding"),
        "Symbol" => Some("Symbol"),
        "Times-Bold" => Some("StandardEncoding"),
        "Times-BoldItalic" => Some("StandardEncoding"),
        "Times-Italic" => Some("StandardEncoding"),
        "Times-Roman" => Some("StandardEncoding"),
        "ZapfDingbats" => Some("ZapfDingbats"),
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
struct FontCacheInner<'c> {
    fonts: HashMap<String, Box<dyn Font + 'c>>,
    #[borrows(fonts)]
    #[covariant]
    ops: HashMap<String, Box<dyn FontOp + 'this>>,
    #[borrows(fonts)]
    #[covariant]
    renders: HashMap<String, Box<dyn GlyphRender + 'this>>,
}

pub struct FontCache<'c>(FontCacheInner<'c>);

/// Split string by capital char.
fn capital_to_space_separated(s: &str) -> Cow<str> {
    let mut rv: SmallVec<[&str; 3]> = SmallVec::new();
    let mut last = 0;
    for (i, c) in s.char_indices() {
        if c.is_uppercase() {
            if i > 0 {
                rv.push(&s[last..i]);
            }
            last = i;
        }
    }
    if last < s.len() {
        rv.push(&s[last..]);
    }
    if rv.len() <= 1 {
        Cow::Borrowed(s)
    } else {
        Cow::Owned(rv.join(" "))
    }
}

impl<'c> FontCache<'c> {
    fn load_true_type_font_from_bytes<'a, 'b>(
        font: FontDict<'a, 'b>,
        bytes: Vec<u8>,
    ) -> AnyResult<TTFParserFont<'a, 'b>> {
        Ok(TTFParserFont {
            typ: font.subtype()?,
            data: bytes,
            font_dict: font,
        })
    }

    fn load_true_type_from_os(desc: &FontDescriptorDict) -> AnyResult<Vec<u8>> {
        let font_name = desc.font_name()?;
        let font_name = normalize_true_type_font_name(font_name);
        let font_name = capital_to_space_separated(&font_name);
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
                .map(|v| Weight(v as u16))
                .unwrap_or(Weight::NORMAL),
            style,
            ..Default::default()
        };
        if let Some(stretch) = desc.font_stretch()? {
            q.stretch = stretch.into();
        }

        let id = SYSTEM_FONTS.query(&q).expect("font not found in system");
        let face = SYSTEM_FONTS.face(id).unwrap();
        assert_eq!(face.index, 0, "Only one face supported");
        match face.source {
            Source::File(ref path) => Ok(std::fs::read(path)?),
            Source::Binary(ref bytes) => Ok(bytes.as_ref().as_ref().to_owned()),
            Source::SharedFile(_, ref bytes) => Ok(bytes.as_ref().as_ref().to_owned()),
        }
    }

    fn load_embed_font_bytes<'a>(
        resolver: &ObjectResolver<'a>,
        s: &Stream<'a>,
    ) -> AnyResult<Vec<u8>> {
        Ok(s.decode(resolver)?.into_owned())
    }

    fn load_ttf_parser_font<'a, 'b>(
        font: FontDict<'a, 'b>,
        desc: FontDescriptorDict<'a, 'b>,
    ) -> AnyResult<TTFParserFont<'a, 'b>> {
        let bytes = match desc.font_file2()? {
            Some(stream) => Self::load_embed_font_bytes(desc.resolver(), stream)?,
            None => {
                let font_name = desc.font_name()?;
                warn!(
                    "font {} not found in file, try to load from system",
                    font_name,
                );
                Self::load_true_type_from_os(&desc)?
            }
        };
        Self::load_true_type_font_from_bytes(font, bytes)
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
        let font_name = f.font_name()?;
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
                if let Some(font_data) = standard_14_type1_font_data(font_name) {
                    font_data.to_owned()
                } else {
                    bail!("Standard 14 type1 font not found: {}", font_name)
                },
            ),
        };
        bytes.shrink_to_fit();
        Type1Font::new(is_cff, bytes, font)
    }

    fn scan_font<'a, 'b>(font: FontDict<'a, 'b>) -> AnyResult<Option<Box<dyn Font + 'c>>>
    where
        'a: 'c,
        'b: 'c,
    {
        match font.subtype()? {
            FontType::TrueType => {
                let tt = font.truetype()?;
                let desc = tt.font_descriptor()?.unwrap();
                Ok(Some(Box::new(Self::load_ttf_parser_font(font, desc)?)))
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

                        Ok(Some(Box::new(Self::load_ttf_parser_font(font, desc)?)))
                    }
                }
            }

            FontType::Type1 => Self::load_type1_font(font.clone())
                .map(|v| Some(Box::new(v) as Box<dyn Font + 'c>))
                .or_else(|err| {
                    info!(
                        "Failed to load type1 font \"{:?}\", try load as truetype",
                        err
                    );
                    let desc = font.font_descriptor()?.unwrap();
                    Ok(Some(Box::new(Self::load_ttf_parser_font(font, desc)?)))
                }),
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

        Ok(Self(FontCacheInner::try_new(
            fonts,
            |fonts| {
                let mut ops = HashMap::with_capacity(fonts.len());
                for (k, v) in fonts {
                    ops.insert(k.clone(), v.create_op()?);
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
        )?))
    }

    pub fn get_font(&self, s: &str) -> Option<&dyn Font> {
        self.0.borrow_fonts().get(s).map(|x| x.as_ref())
    }

    pub fn get_op(&self, s: &str) -> Option<&(dyn FontOp)> {
        self.0.borrow_ops().get(s).map(|x| x.as_ref())
    }

    pub fn get_glyph_render(&self, s: &str) -> Option<&(dyn GlyphRender)> {
        self.0.borrow_renders().get(s).map(|x| x.as_ref())
    }
}

pub trait FontOp {
    /// Decode char codes to chars, possible using some encoding
    fn decode_chars(&self, s: &[u8]) -> Vec<u32>;
    fn char_to_gid(&self, ch: u32) -> u16;
    /// Return glyph width for specified char
    fn char_width(&self, ch: u32) -> u32;
    fn units_per_em(&self) -> u16 {
        1000
    }
}

struct Type0FontOp {
    widths: CIDFontWidths,
    default_width: u32,
}

impl Type0FontOp {
    fn new(font: &Type0FontDict) -> AnyResult<Self> {
        if let NameOrStream::Name(ref encoding) = font.encoding()? {
            assert_eq!(encoding.as_ref(), "Identity-H");
            // assert_eq!(encoding.as_ref(), CIDFontEncoding::IdentityH.as_ref());
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

impl FontOp for Type0FontOp {
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
        ch as u16
    }

    fn char_width(&self, ch: u32) -> u32 {
        self.widths.char_width(ch).unwrap_or(self.default_width)
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

impl<'a, 'b> Font for CIDFontType0Font<'a, 'b> {
    fn font_type(&self) -> FontType {
        FontType::Type0
    }

    fn create_op(&self) -> AnyResult<Box<dyn FontOp + '_>> {
        Ok(Box::new(Type0FontOp::new(&self.font_dict.type0()?)?))
    }

    fn create_glyph_render(&self) -> AnyResult<Box<dyn GlyphRender + '_>> {
        Ok(Box::new(Type1GlyphRender { font: &self.font }))
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

        assert_eq!(100, font_width.char_width('a' as u32));
        assert_eq!(200, font_width.char_width('b' as u32));
        assert_eq!(400, font_width.char_width('d' as u32));
        assert_eq!(15, font_width.char_width('e' as u32));
    }

    #[test_case("" => "")]
    #[test_case("foo" => "foo")]
    #[test_case("Bar" => "Bar")]
    #[test_case("FooBar" => "Foo Bar")]
    fn test_split_by_capital(s: &str) -> String {
        capital_to_space_separated(s).into_owned()
    }

    #[test_case("s" => "s"; "no need to normalize")]
    #[test_case("TimesNewRomanPSMT" => "TimesNewRoman"; "PSMT")]
    fn test_normalize_true_type_font_name(s: &str) -> String {
        normalize_true_type_font_name(s)
    }
}
