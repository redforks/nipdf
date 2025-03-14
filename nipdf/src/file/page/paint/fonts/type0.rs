use super::{
    ChainGlyphAdvance, Font, FontKitFontExt, FontOp, GlyphAdvance, GlyphRender, PathSink,
    TTFGlyphRender, UnitPerEmAdjust,
};
use crate::file::paint::fonts::LengthAdavnce;
use crate::graphics::{NameOrDictByRef, NameOrStream};
use crate::object::PdfObjectCore as _;
use crate::text::{FontDict, Type0FontDict};
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
    fn create_op(&self) -> Result<Box<dyn FontOp + '_>> {
        Ok(Box::new(CIDFontType0FontOp::new(&self.font_dict.type0()?)?))
    }

    fn create_glyph_render(&self) -> Result<Box<dyn GlyphRender<P> + '_>> {
        Ok(Box::new(TTFGlyphRender { font: &self.font }))
    }

    fn create_glyph_width(&self) -> Result<Box<dyn GlyphAdvance + '_>> {
        let font_dict = self.font_dict.type0()?;
        let cid_fonts = font_dict.descendant_fonts()?;
        let cid_font = &cid_fonts[0];

        // Check if we need to use vertical metrics
        let encoding = get_font_encoding(&mut CMapRegistry::new(), &font_dict)?;
        let write_mode = get_write_mode(&encoding);

        // Use w2 for vertical writing mode, w for horizontal
        let (widths, default_width) = if write_mode == WriteMode::Vertical {
            let widths = cid_font.w2()?;
            let default_width = cid_font.dw2()?.1;
            (widths, default_width)
        } else {
            (cid_font.w()?, cid_font.dw()?)
        };

        let units_per_em = self.font.units_per_em()?;
        Ok(Box::new(ChainGlyphAdvance(
            UnitPerEmAdjust::new(units_per_em, widths),
            UnitPerEmAdjust::new(units_per_em, LengthAdavnce(default_width)),
        )))
    }
}

struct CIDFontType0FontOp {
    encoding: Option<Rc<CMap>>,
    write_mode: WriteMode,
}

/// Helper function to determine the write mode from an encoding
fn get_write_mode(encoding: &Option<Rc<CMap>>) -> WriteMode {
    if encoding
        .as_ref()
        .is_none_or(|cmap| cmap.w_mode == WriteMode::Horizontal)
    {
        WriteMode::Horizontal
    } else {
        WriteMode::Vertical
    }
}

impl CIDFontType0FontOp {
    fn new(font: &Type0FontDict<'_, '_>) -> Result<Self> {
        let mut cmap_registry = CMapRegistry::new();
        let encoding = get_font_encoding(&mut cmap_registry, font)?;
        let write_mode = get_write_mode(&encoding);

        Ok(Self {
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
    ttf_face: Option<TTFFace<'a>>,
    units_per_em: u16,
    // Convert as Identity-H if None
    encoding: Option<Rc<CMap>>,
    cid_to_gid: Option<CIDToGIDMap>,
    cid_is_gid: bool,
    write_mode: WriteMode,
}

impl<'a> CIDFontType2FontOp<'a> {
    fn new(
        encoding: Option<Rc<CMap>>,
        is_embed: bool,
        ttf_data: &'a [u8],
        units_per_em: u16,
        cid_to_gid: Option<CIDToGIDMap>,
    ) -> Result<Self> {
        let write_mode = get_write_mode(&encoding);
        let ttf_face = TTFFace::parse(ttf_data, 0).ok();
        let cid_is_gid = is_embed && cid_to_gid.is_none();
        ensure_whatever!(
            cid_is_gid || ttf_face.is_some() || cid_to_gid.is_some(),
            "ttf_face and cid_to_gid can not both be None"
        );

        Ok(Self {
            ttf_face,
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
                glyph_index(
                    self.ttf_face
                        .as_ref()
                        .whatever_context::<_, ObjectValueError>("get ttf_face")?,
                    ch,
                )?
                .map_or_else(
                    || ch.try_into().whatever_context("convert ch to u16 gid"),
                    Ok,
                )
            },
            |m| {
                m.to_gid(ch as usize).map_or_else(
                    || {
                        glyph_index(
                            self.ttf_face
                                .as_ref()
                                .whatever_context::<_, ObjectValueError>("get ttf face")?,
                            ch,
                        )?
                        .map_or_else(
                            || ch.try_into().whatever_context("convert ch to u16 gid"),
                            Ok,
                        )
                    },
                    Ok,
                )
            },
        )
    }

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
    fn create_op(&self) -> Result<Box<dyn FontOp + '_>> {
        // Get the common font info upfront
        let font = self.font_dict.type0()?;
        let units_per_em = self.font.units_per_em()?;
        let mut cmap_registry = CMapRegistry::new();
        let encoding = get_font_encoding(&mut cmap_registry, &font)?;
        let write_mode = if !encoding
            .as_ref()
            .is_none_or(|cmap| cmap.w_mode == WriteMode::Horizontal)
        {
            WriteMode::Vertical
        } else {
            WriteMode::Horizontal
        };

        // Resolve CIDToGIDMap before creating the FontOp
        let cid_to_gid = match font.descendant_fonts()?[0].cid_to_gid_map()? {
            NameOrStream::Name(_) => None,
            NameOrStream::Stream(s) => Some(CIDToGIDMap::new(
                s.decode(font.resolver())
                    .whatever_context::<_, ObjectValueError>("decode stream")?
                    .into_owned(),
            )?),
        };

        // Check encoding and create appropriate FontOp
        if cid_to_gid.is_none() && !self.font_is_embed {
            if let Some(NameOrDictByRef::Name(ref name)) = self.font_dict.encoding()? {
                if *name != &sname("Identity-H")
                    && *name != &sname("Identity-V")
                    && Encoding::predefined(name).is_none()
                {
                    let encoding = match name.as_ref() {
                        "GBK-EUC-H" => encoding_rs::GBK,
                        "EUC-H" => encoding_rs::EUC_JP,
                        "UniJIS-UCS2-HW-H" | "UniGB-UTF16-H" | "UniCNS-UTF16-H" => {
                            encoding_rs::UTF_16BE
                        }
                        "ETenms-B5-V" | "ETenms-B5-H" | "ETen-B5-H" | "B5pc-H" => encoding_rs::BIG5,
                        "90pv-RKSJ-H" | "90ms-RKSJ-H" => encoding_rs::SHIFT_JIS,
                        _ => whatever!("unsupported encoding: '{}'", name),
                    };

                    return Ok(Box::new(CIDFontType2UnicodeFontOp::new(
                        &self.font,
                        units_per_em,
                        encoding,
                        write_mode,
                    )));
                }
            }
        }

        Ok(Box::new(CIDFontType2FontOp::new(
            encoding,
            self.font_is_embed,
            &self.data,
            units_per_em,
            cid_to_gid,
        )?))
    }

    fn create_glyph_render(&self) -> Result<Box<dyn GlyphRender<P> + '_>> {
        // Use FreeType, TTFParser failed render bug1734802.pdf
        Ok(Box::new(TTFGlyphRender { font: &self.font }))
    }

    fn create_glyph_width(&self) -> Result<Box<dyn GlyphAdvance + '_>> {
        let font_dict = self.font_dict.type0()?;
        let cid_fonts = font_dict.descendant_fonts()?;
        let cid_font = &cid_fonts[0];

        // Check if we need to use vertical metrics
        let encoding = get_font_encoding(&mut CMapRegistry::new(), &font_dict)?;
        let write_mode = get_write_mode(&encoding);

        // Use w2 for vertical writing mode, w for horizontal
        let (widths, default_width) = if write_mode == WriteMode::Vertical {
            let widths = cid_font.w2()?;
            let default_width = cid_font.dw2()?.1;
            (widths, default_width)
        } else {
            (cid_font.w()?, cid_font.dw()?)
        };

        let units_per_em = self.font.units_per_em()?;
        Ok(Box::new(ChainGlyphAdvance(
            UnitPerEmAdjust::new(units_per_em, widths),
            UnitPerEmAdjust::new(units_per_em, LengthAdavnce(default_width)),
        )))
    }
}

struct CIDFontType2UnicodeFontOp<'a> {
    face: &'a FontKitFont,
    units_per_em: u16,
    encoding: &'static CharEncoding,
    write_mode: WriteMode,
}

impl<'a> CIDFontType2UnicodeFontOp<'a> {
    fn new(
        face: &'a FontKitFont,
        units_per_em: u16,
        encoding: &'static CharEncoding,
        write_mode: WriteMode,
    ) -> Self {
        Self {
            face,
            units_per_em,
            encoding,
            write_mode,
        }
    }
}

impl<'a> FontOp for CIDFontType2UnicodeFontOp<'a> {
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

        Ok(self
            .face
            .glyph_for_char(c)
            .map(|glyph| glyph as u16)
            .unwrap_or_else(|| {
                warn!("Glyph not found for character: {:?}", c);
                0
            }))
    }

    fn units_per_em(&self) -> Result<u16> {
        Ok(self.units_per_em)
    }

    fn write_mode(&self) -> WriteMode {
        self.write_mode
    }
}
