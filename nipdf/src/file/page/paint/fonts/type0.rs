use super::truetype::glyph_index;
use super::{
    ChainGlyphAdvance, Font, FontKitFontExt, FontOp, GlyphAdvance, GlyphRender, PathSink,
    TTFGlyphRender, UnitPerEmAdjust,
};
use crate::file::paint::fonts::LengthAdavnce;
use crate::graphics::{NameOrDictByRef, NameOrStream};
use crate::object::PdfObjectCore as _;
use crate::text::{CIDFontWidths, FontDict, Type0FontDict};
use crate::{ObjectValueError, Result};
use encoding_rs::Encoding as CharEncoding;
use font_kit::loaders::freetype::Font as FontKitFont;
use log::warn;
use owned_ttf_parser::OwnedFace as OwnedTTFFace;
use prescript::cmap::{CMap, CMapRegistry, WriteMode};
use prescript::{Encoding, Name, name, sname};
use snafu::{OptionExt as _, ResultExt as _, ensure_whatever, whatever};
use std::rc::Rc;
use std::sync::Arc;

/// Font for Type 0 CIDFont, its descendant font is Cff.
pub(super) struct CIDFontType0Font {
    font: FontKitFont,
    font_op: CIDFontType0FontOp,
    width:
        ChainGlyphAdvance<UnitPerEmAdjust<Option<CIDFontWidths>>, UnitPerEmAdjust<LengthAdavnce>>,
}

impl CIDFontType0Font {
    pub fn new(font_dict: &FontDict<'_, '_>, data: Vec<u8>) -> Result<Self> {
        let font = FontKitFont::from_bytes(data.into(), 0)
            .whatever_context::<_, ObjectValueError>("decode FontKitFont for Type0")?;
        let font_op = CIDFontType0FontOp::new(&font_dict.type0()?)?;

        let type0_dict = font_dict.type0()?;
        let cid_fonts = type0_dict.descendant_fonts()?;
        let cid_font = &cid_fonts[0];

        // Check if we need to use vertical metrics
        let encoding = get_font_encoding(&mut CMapRegistry::new(), &type0_dict)?;
        let write_mode = get_write_mode(&encoding);

        // Use w2 for vertical writing mode, w for horizontal
        let (widths, default_width) = if write_mode == WriteMode::Vertical {
            let widths = cid_font.w2()?;
            let default_width = cid_font.dw2()?.1;
            (widths, default_width)
        } else {
            (cid_font.w()?, cid_font.dw()?)
        };

        let units_per_em = font.units_per_em()?;
        let width = ChainGlyphAdvance(
            UnitPerEmAdjust::new(units_per_em, widths),
            UnitPerEmAdjust::new(units_per_em, LengthAdavnce(default_width)),
        );

        Ok(Self {
            font,
            font_op,
            width,
        })
    }
}

impl<P: PathSink + 'static> Font<P> for CIDFontType0Font {
    fn create_op(&self) -> Result<Box<dyn FontOp>> {
        Ok(Box::new(self.font_op.clone()))
    }

    fn create_glyph_render(&self) -> Result<Box<dyn GlyphRender<P>>> {
        Ok(Box::new(TTFGlyphRender {
            font: self.font.clone(),
        }))
    }

    fn create_glyph_width(&self) -> Result<Box<dyn GlyphAdvance>> {
        Ok(Box::new(self.width.clone()))
    }
}

#[derive(Clone)]
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
#[derive(Clone)]
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

struct CIDFontType2FontOp {
    ttf_face: Option<OwnedTTFFace>,
    units_per_em: u16,
    // Convert as Identity-H if None
    encoding: Option<Rc<CMap>>,
    cid_to_gid: Option<CIDToGIDMap>,
    cid_is_gid: bool,
    write_mode: WriteMode,
}

impl CIDFontType2FontOp {
    fn new(
        encoding: Option<Rc<CMap>>,
        is_embed: bool,
        ttf_data: Vec<u8>,
        units_per_em: u16,
        cid_to_gid: Option<CIDToGIDMap>,
    ) -> Result<Self> {
        let write_mode = get_write_mode(&encoding);
        let ttf_face = OwnedTTFFace::from_vec(ttf_data, 0).ok();
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

impl FontOp for CIDFontType2FontOp {
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

pub(super) struct CIDFontType2Font {
    #[allow(clippy::rc_buffer)] // FontKitFont uses Arc<Vec<u8>> type
    data: Arc<Vec<u8>>,
    font: FontKitFont,
    font_is_embed: bool,
    units_per_em: u16,
    encoding: Option<Rc<CMap>>,
    cid_to_gid: Option<CIDToGIDMap>,
    // use unitcode_font_op if encoding_name is Some
    encoding_name: Option<Name>,
    width:
        ChainGlyphAdvance<UnitPerEmAdjust<Option<CIDFontWidths>>, UnitPerEmAdjust<LengthAdavnce>>,
}

impl CIDFontType2Font {
    #[allow(clippy::rc_buffer)] // FontKitFont uses Arc<Vec<u8>> type
    pub fn new(
        font_is_embed: bool,
        data: Arc<Vec<u8>>,
        font_dict: &FontDict<'_, '_>,
    ) -> Result<Self> {
        let font = FontKitFont::from_bytes(data.clone(), 0)
            .whatever_context::<_, ObjectValueError>("decode FontKitFont for Type2")?;
        let units_per_em = font.units_per_em()?;

        let cid_to_gid = match font_dict.type0()?.descendant_fonts()?[0].cid_to_gid_map()? {
            NameOrStream::Name(_) => None,
            NameOrStream::Stream(s) => Some(CIDToGIDMap::new(
                s.decode(font_dict.resolver())
                    .whatever_context::<_, ObjectValueError>("decode stream")?
                    .into_owned(),
            )?),
        };

        let encoding_name = if cid_to_gid.is_none() && !font_is_embed {
            if let Some(NameOrDictByRef::Name(name)) = font_dict.encoding()? {
                (name != &sname("Identity-H")
                    && name != &sname("Identity-V")
                    && Encoding::predefined(name).is_none())
                .then(|| name.clone())
            } else {
                None
            }
        } else {
            None
        };

        // Calculate width metrics
        let type0_dict = font_dict.type0()?;
        let cid_fonts = type0_dict.descendant_fonts()?;
        let cid_font = &cid_fonts[0];

        let encoding = get_font_encoding(&mut CMapRegistry::new(), &type0_dict)?;
        let write_mode = get_write_mode(&encoding);

        let (widths, default_width) = if write_mode == WriteMode::Vertical {
            let widths = cid_font.w2()?;
            let default_width = cid_font.dw2()?.1;
            (widths, default_width)
        } else {
            (cid_font.w()?, cid_font.dw()?)
        };

        let width = ChainGlyphAdvance(
            UnitPerEmAdjust::new(units_per_em, widths),
            UnitPerEmAdjust::new(units_per_em, LengthAdavnce(default_width)),
        );

        Ok(Self {
            data,
            font,
            font_is_embed,
            units_per_em,
            encoding,
            cid_to_gid,
            encoding_name,
            width,
        })
    }
}

impl<P: PathSink + 'static> Font<P> for CIDFontType2Font {
    fn create_op(&self) -> Result<Box<dyn FontOp>> {
        // Get the common font info upfront
        let write_mode = if !self
            .encoding
            .as_ref()
            .is_none_or(|cmap| cmap.w_mode == WriteMode::Horizontal)
        {
            WriteMode::Vertical
        } else {
            WriteMode::Horizontal
        };

        // Check encoding and create appropriate FontOp
        if let Some(name) = &self.encoding_name {
            let encoding = match name.as_str() {
                "GBK-EUC-H" => encoding_rs::GBK,
                "EUC-H" => encoding_rs::EUC_JP,
                "UniJIS-UCS2-HW-H" | "UniGB-UTF16-H" | "UniCNS-UTF16-H" => encoding_rs::UTF_16BE,
                "ETenms-B5-V" | "ETenms-B5-H" | "ETen-B5-H" | "B5pc-H" => encoding_rs::BIG5,
                "90pv-RKSJ-H" | "90ms-RKSJ-H" => encoding_rs::SHIFT_JIS,
                _ => whatever!("unsupported encoding: '{}'", name),
            };

            return Ok(Box::new(CIDFontType2UnicodeFontOp::new(
                self.font.clone(),
                self.units_per_em,
                encoding,
                write_mode,
            )));
        }

        Ok(Box::new(CIDFontType2FontOp::new(
            self.encoding.clone(),
            self.font_is_embed,
            self.data.as_ref().clone(),
            self.units_per_em,
            self.cid_to_gid.clone(),
        )?))
    }

    fn create_glyph_render(&self) -> Result<Box<dyn GlyphRender<P>>> {
        // Use FreeType, TTFParser failed render bug1734802.pdf
        Ok(Box::new(TTFGlyphRender {
            font: self.font.clone(),
        }))
    }

    fn create_glyph_width(&self) -> Result<Box<dyn GlyphAdvance>> {
        Ok(Box::new(self.width.clone()))
    }
}

struct CIDFontType2UnicodeFontOp {
    face: FontKitFont,
    units_per_em: u16,
    encoding: &'static CharEncoding,
    write_mode: WriteMode,
}

impl CIDFontType2UnicodeFontOp {
    fn new(
        face: FontKitFont,
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
            .map_or_else(
                || {
                    warn!("Glyph not found for character: {:?}", c);
                    0
                },
                |glyph| glyph,
            )
            .try_into()
            .whatever_context::<_, ObjectValueError>("failed to get glyph id")
    }

    fn units_per_em(&self) -> Result<u16> {
        Ok(self.units_per_em)
    }

    fn write_mode(&self) -> WriteMode {
        self.write_mode
    }
}
