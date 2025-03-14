use super::{
    ChainGlyphAdvance, EncodingParser, FirstLastFontWidth, FontKitFontExt as _, FontOp,
    FreeTypeFontWidth, GlyphRender, PathSink, TTFGlyphRender, UnitPerEmAdjust,
};
use crate::{
    ObjectValueError, Result,
    file::page::paint::fonts::{Font, FontDict},
};
use font_kit::loaders::freetype::Font as FontKitFont;
use log::warn;
use phf::phf_map;
use prescript::cmap::WriteMode;
use snafu::OptionExt as _;
use snafu::ResultExt;
use std::sync::Arc;
use ttf_parser::Face as TTFFace;

pub struct TTFFont<'a> {
    font_dict: FontDict<'a, 'a>,
    face: FontKitFont,
    data: Arc<Vec<u8>>,
}

impl<'a> TTFFont<'a> {
    pub fn new(data: Arc<Vec<u8>>, font_dict: FontDict<'a, 'a>) -> Result<Self> {
        let face = FontKitFont::from_bytes(data.clone(), 0)
            .whatever_context::<_, ObjectValueError>("parse TTF Font")?;
        Ok(Self {
            font_dict,
            face,
            data,
        })
    }
}

impl<P: PathSink> Font<P> for TTFFont<'_> {
    fn create_op(&self) -> Result<Box<dyn FontOp + '_>> {
        let encoding = EncodingParser(&self.font_dict).ttf()?;
        Ok(Box::new(TTFFontOp::new(
            &self.face,
            encoding,
            TTFFace::parse(&self.data, 0)
                .whatever_context::<_, ObjectValueError>("parse TTF Font")?,
            self.face.units_per_em()?,
        )))
    }

    fn create_glyph_render(&self) -> Result<Box<dyn GlyphRender<P>>> {
        Ok(Box::new(TTFGlyphRender {
            font: self.face.clone(),
        }))
    }

    fn create_glyph_width(&self) -> Result<Box<dyn super::GlyphAdvance>> {
        let units_per_em = self.face.units_per_em()?;
        let first_last_width =
            UnitPerEmAdjust::new(units_per_em, FirstLastFontWidth::from(&self.font_dict)?);
        let free_type_width = FreeTypeFontWidth::new(
            self.face.clone(),
            /* TrueType font no Vertical mode*/ WriteMode::Horizontal,
        );

        Ok(Box::new(ChainGlyphAdvance(
            first_last_width,
            free_type_width,
        )))
    }
}

static GLYPH_NAME_TO_UNICODE: phf::Map<&'static str, u32> = include!("../glyph_name_to_unicode.rs");

pub struct TTFFontOp<'a> {
    face: &'a FontKitFont,
    units_per_em: u16,
    encoding: Option<prescript::Encoding>,
    ttf_font: TTFFace<'a>,
}

impl<'a> TTFFontOp<'a> {
    pub fn new(
        face: &'a FontKitFont,
        encoding: Option<prescript::Encoding>,
        ttf_font: TTFFace<'a>,
        units_per_em: u16,
    ) -> Self {
        Self {
            units_per_em,
            face,
            encoding,
            ttf_font,
        }
    }
}

impl FontOp for TTFFontOp<'_> {
    fn decode_chars(&self, s: &[u8]) -> Result<Vec<u32>, ObjectValueError> {
        Ok(s.iter().map(|v| *v as u32).collect())
    }

    fn char_to_gid(&self, mut ch: u32) -> Result<u16, ObjectValueError> {
        if let Some(encoding) = self.encoding.as_ref() {
            let glyph_name = encoding.get_str(
                ch.try_into()
                    .whatever_context::<_, ObjectValueError>("Convert ch to u8")?,
            );
            if glyph_name != prescript::NOTDEF {
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

    fn units_per_em(&self) -> Result<u16, ObjectValueError> {
        Ok(self.units_per_em)
    }

    fn write_mode(&self) -> WriteMode {
        WriteMode::Horizontal
    }
}

fn glyph_index(ttf_font: &TTFFace<'_>, ch: u32) -> Result<Option<u16>, ObjectValueError> {
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
