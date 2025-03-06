use crate::{
    ObjectValueError, Result,
    file::{ObjectResolver, page::ResourceDict},
    graphics::{Operation, Point, parse_operations, trans::GlyphLength},
    object::{Dictionary, Object, PdfObject, PdfObjectCore as _, Stream},
    text::{
        CIDFontType, FontDescriptorDict, FontDescriptorFlags, FontDict, FontType, Type3FontDict,
    },
};
use encoding::EncodingParser;
use font_kit::{hinting::HintingOptions, loaders::freetype::Font as FontKitFont};
use fontdb::{Database, Family, Query, Source, Weight};
use heck::ToTitleCase;
use log::{info, warn};
use num_traits::ToPrimitive;
use ouroboros::self_referencing;
use pathfinder_geometry::{line_segment::LineSegment2F, vector::Vector2F};
use prescript::{
    Encoding, Name,
    cmap::{CMapRegistry, WriteMode},
    sname,
};
use snafu::{OptionExt, ResultExt, ensure_whatever, whatever};
use std::{
    collections::HashMap,
    ops::RangeInclusive,
    sync::{Arc, LazyLock},
};
use type1::{Type1Font, Type1FontOp};

mod encoding;
mod truetype;
mod type0;
mod type1;
mod type3;

/// FontWidth used in Type1 and TrueType fonts
struct FirstLastFontWidth {
    range: RangeInclusive<u32>,
    widths: Vec<u32>,
    default_width: u32,
}

impl GlyphAdvance for FirstLastFontWidth {
    fn advance(&self, gid: u32) -> Result<GlyphLength> {
        Ok(GlyphLength::new(if self.range.contains(&gid) {
            let idx = (gid - self.range.start()) as usize;
            self.widths
                .get(idx)
                .map(|v| *v)
                .unwrap_or(self.default_width)
        } else {
            self.default_width
        } as f32))
    }
}

struct DefaultAdvance(f32);

impl DefaultAdvance {
    pub fn new(width: u32) -> Self {
        Self(width as f32)
    }
}

impl GlyphAdvance for DefaultAdvance {
    fn advance(&self, _gid: u32) -> Result<GlyphLength> {
        Ok(GlyphLength::new(self.0))
    }
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
        if default_width == 0 && widths.len() == 0 {
            return Ok(None);
        }

        let range = first_char.whatever_context::<_, ObjectValueError>("get first_char")?
            ..=last_char.whatever_context::<_, ObjectValueError>("get last_char")?;
        Ok(Some(Self {
            range,
            default_width,
            widths,
        }))
    }
}

struct FreeTypeFontWidth<'a> {
    font: &'a FontKitFont,
}

impl<'a> FreeTypeFontWidth<'a> {
    fn new(font: &'a FontKitFont) -> Self {
        Self { font }
    }
}

impl<'a> GlyphAdvance for FreeTypeFontWidth<'a> {
    fn advance(&self, gid: u32) -> Result<GlyphLength> {
        let r = self
            .font
            .advance(gid)
            .whatever_context::<_, ObjectValueError>("get gid advance")?
            .x()
            .to_u32()
            .whatever_context::<_, ObjectValueError>("convert advance to u32")?;
        Ok(GlyphLength::new(r as f32))
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
    fn render(&self, gid: u16, sink: &mut P);
}

struct TTFGlyphRender<'a> {
    font: &'a FontKitFont,
}

impl<P: PathSink> GlyphRender<P> for TTFGlyphRender<'_> {
    fn render(&self, gid: u16, sink: &mut P) {
        if let Err(e) = self
            .font
            .outline(gid as u32, HintingOptions::None, &mut PathSinkWrap(sink))
        {
            warn!("Failed to render glyph: {}", e);
        }
    }
}

pub trait Font<P> {
    fn create_op(&self, cmap_registry: &mut CMapRegistry) -> Result<Box<dyn FontOp + '_>>;
    fn create_glyph_render(&self) -> Result<Box<dyn GlyphRender<P> + '_>>;
    fn create_glyph_width(&self) -> Result<Box<dyn GlyphAdvance + '_>>;
    fn as_type3(&self) -> Option<&type3::Type3Font<'_, '_>> {
        None
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
        let font = FontKitFont::from_bytes(font_data, 0)
            .whatever_context::<_, ObjectValueError>("create FontKitFont for fallback")?;

        Ok(Self { font })
    }

    pub fn create_fallback_op(&self) -> Box<dyn FontOp + '_> {
        Box::new(Type1FontOp::new_fallback(&self.font))
    }
}

impl<P: PathSink> Font<P> for FallbackFont {
    fn create_op(&self, _cmap_registry: &mut CMapRegistry) -> Result<Box<dyn FontOp + '_>> {
        Ok(Box::new(Type1FontOp::new_fallback(&self.font)))
    }

    fn create_glyph_render(&self) -> Result<Box<dyn GlyphRender<P> + '_>> {
        Ok(Box::new(TTFGlyphRender { font: &self.font }))
    }

    fn create_glyph_width(&self) -> Result<Box<dyn GlyphAdvance + '_>> {
        todo!()
    }
}

static SYSTEM_FONTS: LazyLock<Database> = LazyLock::new(|| {
    let mut db = Database::new();
    db.load_system_fonts();
    // set fallback font that support Cjk, depends on specific environment,
    // TODO: better way to provide default fonts
    db.set_serif_family("Noto Serif CJK SC");
    db.set_sans_serif_family("Noto Sans CJK SC");
    db.set_monospace_family("Noto Sans Mono CJK SC");
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
    fallback_font: FallbackFont,

    #[borrows(fonts, mut cmap_registry)]
    #[covariant]
    ops: HashMap<Name, Box<dyn FontOp + 'this>>,
    #[borrows(fonts)]
    #[covariant]
    renders: HashMap<Name, Box<dyn GlyphRender<P> + 'this>>,
    #[borrows(fonts)]
    #[covariant]
    glyph_widths: HashMap<Name, Box<dyn GlyphAdvance + 'this>>,

    #[borrows(fallback_font)]
    #[covariant]
    fallback_op: Box<dyn FontOp + 'this>,
    #[borrows(fallback_font)]
    #[covariant]
    fallback_render: Box<dyn GlyphRender<P> + 'this>,
    #[borrows(fallback_font)]
    #[covariant]
    fallback_width: Box<dyn GlyphAdvance + 'this>,
}

pub struct FontCache<'c, P: PathSink + 'static> {
    cache: FontCacheInner<'c, P>,
}

impl<'c, P: PathSink + 'static> FontCache<'c, P> {
    fn load_true_type_from_os(desc: &FontDescriptorDict<'_, '_>) -> Result<Vec<u8>> {
        let font_name = desc.font_name()?;
        let font_name = normalize_true_type_font_name(&font_name);
        let font_name_title_case = font_name.to_title_case();
        let mut families = vec![
            Family::Name(font_name.as_ref()),
            Family::Name(&font_name_title_case),
        ];
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
            Ok(Box::new(type0::CIDFontType2Font::new(
                is_embed, ttf_bytes, font,
            )?))
        } else {
            Ok(Box::new(truetype::TTFFont::new(ttf_bytes, font)?))
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
                        Ok(Some(Box::new(type0::CIDFontType0Font::new(
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
                    let desc = font.font_descriptor()?;
                    Ok(Some(Self::load_ttf_parser_font(
                        FontType::Type1,
                        font,
                        desc.as_ref(),
                    )?))
                }),

            FontType::Type3 => Ok(Some(Box::new(type3::Type3Font::new(font)?))),
            _ => {
                #[cfg(debug_assertions)]
                todo!("Unsupported font type: {:?}", font.subtype()?);
                #[cfg(not(debug_assertions))]
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
            let font = match Self::scan_font(v) {
                Ok(Some(font)) => font,
                Ok(None) => {
                    warn!("Font {} is not supported, use fallback font", k);
                    continue;
                }
                Err(e) => {
                    warn!("Failed to load font {}: {}, use fallback font", k, e);
                    continue;
                }
            };
            fonts.insert(k, font);
        }

        if fonts.is_empty() {
            warn!("No fonts found, use fallback font");
        }

        Ok(Self {
            cache: FontCacheInner::try_new(
                fonts,
                CMapRegistry::new(),
                FallbackFont::new().unwrap(),
                |fonts, cmap_registry| {
                    let mut ops = HashMap::with_capacity(fonts.len());
                    for (k, v) in fonts {
                        ops.insert(
                            k.clone(),
                            v.create_op(cmap_registry)
                                .with_whatever_context::<_, _, ObjectValueError>(|_| {
                                    format!("Create FontOp for: {}", k)
                                })?,
                        );
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
                |fonts| {
                    let mut renders = HashMap::with_capacity(fonts.len());
                    for (k, v) in fonts {
                        renders.insert(k.clone(), v.create_glyph_width()?);
                    }
                    Ok(renders)
                },
                |fallback_font| Ok(fallback_font.create_fallback_op()),
                |fallback_font| fallback_font.create_glyph_render(),
                |fallback_font| Font::<P>::create_glyph_width(fallback_font),
            )?,
        })
    }

    pub fn get_font(&self, s: &Name) -> &dyn Font<P> {
        self.cache
            .borrow_fonts()
            .get(s)
            .map_or_else(|| self.cache.borrow_fallback_font(), AsRef::as_ref)
    }

    pub fn get_op(&self, s: &Name) -> &(dyn FontOp) {
        self.cache
            .borrow_ops()
            .get(s)
            .map_or_else(|| self.cache.borrow_fallback_op().as_ref(), AsRef::as_ref)
    }

    pub fn get_glyph_render(&self, s: &Name) -> &(dyn GlyphRender<P>) {
        self.cache.borrow_renders().get(s).map_or_else(
            || self.cache.borrow_fallback_render().as_ref(),
            AsRef::as_ref,
        )
    }

    pub fn get_glyph_width(&self, s: &Name) -> &(dyn GlyphAdvance) {
        self.cache.borrow_glyph_widths().get(s).map_or_else(
            || self.cache.borrow_fallback_width().as_ref(),
            AsRef::as_ref,
        )
    }
}

pub trait GlyphAdvance {
    /// Return glyph width or height based on write_mode
    fn advance(&self, gid: u32) -> Result<GlyphLength>;
}

pub trait FontOp {
    /// Decode char codes to chars, possible using some encoding
    fn decode_chars(&self, s: &[u8]) -> Result<Vec<u32>>;
    fn char_to_gid(&self, ch: u32) -> Result<u16>;
    fn write_mode(&self) -> WriteMode {
        WriteMode::Horizontal
    }
    fn units_per_em(&self) -> Result<u16> {
        Ok(1000)
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

        assert_eq!(100.0, font_width.advance('a' as u32).unwrap().0);
        assert_eq!(200.0, font_width.advance('b' as u32).unwrap().0);
        assert_eq!(400.0, font_width.advance('d' as u32).unwrap().0);
        assert_eq!(15.0, font_width.advance('e' as u32).unwrap().0);
    }

    #[test_case("s" => "s"; "no need to normalize")]
    #[test_case("TimesNewRomanPSMT" => "TimesNewRoman"; "PSMT")]
    fn test_normalize_true_type_font_name(s: &str) -> String {
        normalize_true_type_font_name(s)
    }
}
