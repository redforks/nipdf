use std::sync::Arc;

use super::{
    ChainGlyphAdvance, EncodingParser, FirstLastFontWidth, Font, FontKitFont, FontKitFontExt,
    FontOp, FreeTypeFontWidth, GlyphAdvance, GlyphRender, PathSink, TTFGlyphRender,
    UnitPerEmAdjust,
};
use crate::text::FontDict;
use crate::{ObjectValueError, Result};
use log::info;
use prescript::Encoding;
use prescript::cmap::WriteMode;
use snafu::ResultExt as _;

/// Font implementation using free-type/(font-kit), to handle Type1 fonts
pub(super) struct Type1Font {
    font: FontKitFont,
    encoding: Encoding,
    first_last_width: Option<FirstLastFontWidth>,
}

impl Type1Font {
    pub fn new(is_cff: bool, data: Vec<u8>, font_dict: &FontDict<'_, '_>) -> Result<Self> {
        debug_assert_eq!(data.capacity(), data.len());
        let data = Arc::new(data);
        let font = FontKitFont::from_bytes(data.clone(), 0)
            .whatever_context::<_, ObjectValueError>("create FontKitFont")?;
        let encoding = EncodingParser(font_dict).type1(is_cff, data.as_slice())?;
        let first_last_width = FirstLastFontWidth::from(font_dict)?;
        Ok(Self {
            font,
            encoding,
            first_last_width,
        })
    }
}

impl<P: PathSink> Font<P> for Type1Font {
    fn create_op(&self) -> Result<Box<dyn FontOp>> {
        let units_per_em = self.font.units_per_em()?;
        Ok(Box::new(Type1FontOp::new(
            self.font.clone(),
            self.encoding.clone(),
            units_per_em,
        )))
    }

    fn create_glyph_render(&self) -> Result<Box<dyn GlyphRender<P>>> {
        Ok(Box::new(TTFGlyphRender {
            font: self.font.clone(),
        }))
    }

    fn create_glyph_width(&self) -> Result<Box<dyn GlyphAdvance>> {
        Ok(Box::new(ChainGlyphAdvance(
            UnitPerEmAdjust::new(self.font.units_per_em()?, self.first_last_width.clone()),
            FreeTypeFontWidth::new(
                self.font.clone(),
                /* Type1 font no Vertical mode */ WriteMode::Horizontal,
            ),
        )))
    }
}

pub(super) struct Type1FontOp {
    font: FontKitFont,
    encoding: Encoding,
    units_per_em: u16,
}

impl Type1FontOp {
    fn new(font: FontKitFont, encoding: Encoding, units_per_em: u16) -> Self {
        Self {
            font,
            encoding,
            units_per_em,
        }
    }

    pub fn new_fallback(font: FontKitFont) -> Self {
        #[allow(clippy::unwrap_used)] // this won't happen
        let units_per_em = font.metrics().units_per_em.try_into().unwrap();
        Self {
            font,
            encoding: Encoding::WIN_ANSI,
            units_per_em,
        }
    }
}

impl FontOp for Type1FontOp {
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

    fn units_per_em(&self) -> Result<u16> {
        Ok(self.units_per_em)
    }
}
