use super::{
    ChainGlyphAdvance, EncodingParser, FirstLastFontWidth, Font, FontKitFont, FontKitFontExt,
    FontOp, FreeTypeFontWidth, GlyphRender, PathSink, TTFGlyphRender, UnitPerEmAdjust,
};
use crate::text::FontDict;
use crate::{ObjectValueError, Result};
use log::info;
use prescript::Encoding;
use prescript::cmap::{CMapRegistry, WriteMode};
use snafu::ResultExt as _;

/// Font implementation using free-type/(font-kit), to handle Type1 fonts
pub(super) struct Type1Font<'a> {
    font_data: Vec<u8>,
    is_cff: bool,
    font: FontKitFont,
    font_dict: FontDict<'a, 'a>,
}

impl<'a> Type1Font<'a> {
    pub fn new(is_cff: bool, data: Vec<u8>, font_dict: FontDict<'a, 'a>) -> Result<Self> {
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

    fn create_glyph_width(&self) -> Result<Box<dyn super::GlyphAdvance + '_>> {
        Ok(Box::new(ChainGlyphAdvance(
            UnitPerEmAdjust::new(
                self.font.units_per_em()?,
                FirstLastFontWidth::from(&self.font_dict)?,
            ),
            FreeTypeFontWidth::new(
                &self.font,
                /* Type1 font no Vertical mode*/ WriteMode::Horizontal,
            ),
        )))
    }
}

pub(super) struct Type1FontOp<'a> {
    font: &'a FontKitFont,
    encoding: Encoding,
    units_per_em: u16,
}

impl<'a> Type1FontOp<'a> {
    fn new(
        font_dict: &FontDict<'_, '_>,
        font: &'a FontKitFont,
        is_cff: bool,
        font_data: &'a [u8],
    ) -> Result<Self> {
        let encoding = EncodingParser(font_dict).type1(is_cff, font_data)?;

        let units_per_em = font
            .metrics()
            .units_per_em
            .try_into()
            .whatever_context::<_, ObjectValueError>("convert units_per_em to u16")?;
        Ok(Self {
            font,
            encoding,
            units_per_em,
        })
    }

    pub fn new_fallback(font: &'a FontKitFont) -> Self {
        let units_per_em = font.metrics().units_per_em.try_into().unwrap();
        Self {
            font,
            encoding: Encoding::WIN_ANSI,
            units_per_em,
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

    fn units_per_em(&self) -> Result<u16> {
        Ok(self.units_per_em)
    }
}
