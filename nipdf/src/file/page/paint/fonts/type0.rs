use super::{Font, FontOp, GlyphRender, PathSink, TTFGlyphRender};
use crate::graphics::trans::GlyphLength;
use crate::graphics::{NameOrDictByRef, NameOrStream};
use crate::object::PdfObjectCore as _;
use crate::text::{CIDFontWidths, FontDict, Type0FontDict};
use crate::{ObjectValueError, Result};
use encoding_rs::Encoding as CharEncoding;
use font_kit::loaders::freetype::Font as FontKitFont;
use log::warn;
use prescript::cmap::{CMap, CMapRegistry, WriteMode};
use prescript::{Encoding, name, sname};
use snafu::{OptionExt as _, ResultExt as _, ensure_whatever, whatever};
use std::rc::Rc;
use std::sync::Arc;
use ttf_parser::Face as TTFFace;

/// Font for Type 0 CIDFont, its descendant font is Cff.
pub(super) struct CIDFontType0Font<'a, 'b> {
    font_dict: FontDict<'a, 'b>,
    font: FontKitFont,
}

impl<'a, 'b> CIDFontType0Font<'a, 'b> {
    pub fn new(font_dict: FontDict<'a, 'b>, data: Vec<u8>) -> Result<Self> {
        let font = FontKitFont::from_bytes(data.into(), 0)
            .whatever_context::<_, ObjectValueError>("decode FontKitFont for Type0")?;
        Ok(Self { font_dict, font })
    }
}

impl<P: PathSink + 'static> Font<P> for CIDFontType0Font<'_, '_> {
    fn create_op(&self, _cmap_registry: &mut CMapRegistry) -> Result<Box<dyn FontOp + '_>> {
        Ok(Box::new(CIDFontType0FontOp::new(&self.font_dict.type0()?)?))
    }

    fn create_glyph_render(&self) -> Result<Box<dyn GlyphRender<P> + '_>> {
        Ok(Box::new(TTFGlyphRender { font: &self.font }))
    }

    fn create_glyph_width(&self) -> Result<Box<dyn super::GlyphAdvance + '_>> {
        todo!()
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
        let encoding = get_font_encoding(&mut cmap_registry, font)?;
        let cid_fonts = font.descendant_fonts()?;
        let cid_font = &cid_fonts[0];
        let (write_mode, widths, default_advance) = if !encoding
            .as_ref()
            .is_none_or(|cmap| cmap.w_mode == WriteMode::Horizontal)
        {
            // Vertical mode - try w2() first, fall back to w()
            let widths = match cid_font.w2()? {
                Some(w) => Some(w),
                None => cid_font.w()?,
            };

            // For dw2, it returns Option<(f32, f32)>, but we only need the first component
            let default_advance = match cid_font.dw2()? {
                Some((height, _)) => height as u32,
                None => cid_font.dw().unwrap_or(1000),
            };

            (WriteMode::Vertical, widths, default_advance)
        } else {
            // Horizontal mode - use standard widths
            (WriteMode::Horizontal, cid_font.w()?, cid_font.dw()?)
        };

        Ok(Self {
            widths,
            default_advance,
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

    // fn char_advance(&self, ch: u32) -> Result<GlyphLength> {
    //     let char_width = self
    //         .widths
    //         .as_ref()
    //         .map(|w| w.char_width(ch))
    //         .transpose()
    //         .whatever_context::<_, ObjectValueError>("get char width")?
    //         .flatten()
    //         .unwrap_or(self.default_advance) as f32;
    //     Ok(GlyphLength::new(char_width))
    // }

    fn write_mode(&self) -> WriteMode {
        self.write_mode
    }
}

fn get_font_encoding(
    cmap_registry: &mut CMapRegistry,
    font: &Type0FontDict<'_, '_>,
) -> Result<Option<Rc<CMap>>> {
    match font.encoding()? {
        NameOrStream::Name(encoding_name) => {
            if encoding_name == "Identity-H" {
                Ok(None)
            } else {
                Ok(Some(
                    cmap_registry
                        .get(&name(encoding_name))
                        .whatever_context::<_, ObjectValueError>("Get cmap")?
                        .whatever_context::<_, ObjectValueError>("Get cmap")?,
                ))
            }
        }
        NameOrStream::Stream(s) => {
            let data = s
                .decode(font.resolver())
                .whatever_context::<_, ObjectValueError>("decode cmap from stream")?;
            Ok(Some(
                cmap_registry
                    .add_cmap_file(data.as_ref())
                    .whatever_context::<_, ObjectValueError>("add cmap file")?,
            ))
        }
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
        let encoding = get_font_encoding(cmap_registry, &font)?;
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

    // fn char_advance(&self, ch: u32) -> Result<GlyphLength> {
    //     let mut char_width = self
    //         .widths
    //         .as_ref()
    //         .map(|w| w.char_width(ch))
    //         .transpose()
    //         .whatever_context::<_, ObjectValueError>("get char width")?
    //         .flatten()
    //         .unwrap_or(self.default_advance) as f32;
    //     if self.units_per_em != 1000 {
    //         char_width = char_width / 1000.0 * self.units_per_em as f32;
    //     }
    //     Ok(GlyphLength::new(char_width))
    // }

    fn units_per_em(&self) -> Result<u16> {
        Ok(self.units_per_em)
    }

    fn write_mode(&self) -> WriteMode {
        self.write_mode
    }
}

pub(super) struct CIDFontType2Font<'a, 'b> {
    data: Arc<Vec<u8>>,
    font: FontKitFont,
    font_dict: FontDict<'a, 'b>,
    font_is_embed: bool,
}

impl<'a, 'b> CIDFontType2Font<'a, 'b> {
    pub fn new(
        font_is_embed: bool,
        data: Arc<Vec<u8>>,
        font_dict: FontDict<'a, 'b>,
    ) -> Result<Self> {
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
    fn create_op(&self, cmap_registry: &mut CMapRegistry) -> Result<Box<dyn FontOp + '_>> {
        let face = FontKitFont::from_bytes(self.data.clone(), 0)
            .whatever_context::<_, ObjectValueError>("decode FontKitFont for Type2")?;

        // Get the common font info upfront
        let font = self.font_dict.type0()?;
        let encoding = get_font_encoding(cmap_registry, &font)?;
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
                let encoding = match name.as_ref() {
                    "GBK-EUC-H" => encoding_rs::GBK,
                    "UniJIS-UCS2-HW-H" | "UniGB-UTF16-H" => encoding_rs::UTF_16BE,
                    "ETenms-B5-V" | "ETenms-B5-H" => encoding_rs::BIG5,
                    "90pv-RKSJ-H" => encoding_rs::SHIFT_JIS,
                    _ => whatever!("unsupported encoding: '{}'", name),
                };

                return Ok(Box::new(CIDFontType2UnicodeFontOp::new(
                    face,
                    widths,
                    default_advance,
                    units_per_em,
                    encoding,
                    write_mode,
                )));
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

    fn create_glyph_width(&self) -> Result<Box<dyn super::GlyphAdvance + '_>> {
        todo!()
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

    // fn char_advance(&self, ch: u32) -> Result<GlyphLength> {
    //     let mut char_width = self
    //         .width
    //         .as_ref()
    //         .map(|w| w.char_width(ch))
    //         .transpose()
    //         .whatever_context::<_, ObjectValueError>("get char width")?
    //         .flatten()
    //         .unwrap_or(self.default_advance) as f32;
    //     if self.units_per_em != 1000 {
    //         char_width = char_width / 1000.0 * self.units_per_em as f32;
    //     }
    //     Ok(GlyphLength::new(char_width))
    // }

    fn units_per_em(&self) -> Result<u16> {
        Ok(self.units_per_em)
    }

    fn write_mode(&self) -> WriteMode {
        self.write_mode
    }
}
