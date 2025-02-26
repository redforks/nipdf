use std::rc::Rc;

use crate::graphics::NameOrStream;
use crate::graphics::trans::GlyphLength;
use crate::object::PdfObjectCore as _;
use crate::text::{CIDFontWidths, FontDict, FontType, Type0FontDict};
use crate::{ObjectValueError, Result};
use font_kit::loaders::freetype::Font as FontKitFont;
use prescript::cmap::{CMap, CMapRegistry, WriteMode};
use prescript::name;
use snafu::{OptionExt as _, ResultExt as _, ensure_whatever};

use super::{Font, FontOp, GlyphRender, PathSink, TTFGlyphRender};

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
