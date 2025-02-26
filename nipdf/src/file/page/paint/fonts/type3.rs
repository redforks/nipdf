use super::{
    Encoding, FirstLastFontWidth, Font, FontDict, GlyphLength, GlyphRender, Operation, PathSink,
    parse_operations,
};
use crate::file::page::paint::fonts::FontOp;
use crate::{
    ObjectValueError, Result, file::page::ResourceDict, graphics::trans::GlyphToTextSpace,
    object::PdfObjectCore as _,
};
use log::info;
use num_traits::ToPrimitive;
use prescript::Name;
use snafu::{OptionExt, ResultExt};
use std::collections::HashMap;
use winnow::{Parser as _, combinator::terminated, token::rest};

pub struct Type3Font<'a, 'b> {
    name_to_gid: HashMap<Name, u16>,
    glyphs: Box<[Type3Glyph]>,
    dict: FontDict<'a, 'b>,
}

impl<'a, 'b> Type3Font<'a, 'b> {
    fn parse_glyphs(d: &super::Type3FontDict<'_, '_>) -> Result<Vec<(Name, Type3Glyph)>> {
        let procs = d.char_procs()?;
        let mut r = Vec::with_capacity(procs.len());
        for (name, stream) in &procs {
            info!("parse Type3 glyph: {}", name.as_str());
            let data = stream
                .decode(d.resolver())
                .whatever_context::<_, ObjectValueError>("decode stream")?;
            let ops = terminated(parse_operations::<crate::ParserError>, rest)
                .parse(&data[..])
                .map_err(winnow::error::ParseError::into_inner)
                .whatever_context::<_, ObjectValueError>("parse type3 operation")?;
            r.push((name.clone(), Type3Glyph(ops.into())));
        }

        Ok(r)
    }

    pub fn new(dict: FontDict<'a, 'b>) -> Result<Self> {
        let type3 = dict.type3()?;
        let glyph_and_names = Self::parse_glyphs(&type3)?;
        let mut glyphs = Vec::with_capacity(glyph_and_names.len());
        let mut glyph_ids = HashMap::with_capacity(glyph_and_names.len());
        for (name, glyph) in glyph_and_names {
            let gid = glyphs
                .len()
                .try_into()
                .whatever_context::<_, ObjectValueError>("glyphs length convert to u16")?;
            glyphs.push(glyph);
            glyph_ids.insert(name, gid);
        }

        Ok(Self {
            name_to_gid: glyph_ids,
            glyphs: glyphs.into(),
            dict,
        })
    }

    pub fn resources(&self) -> Result<Option<ResourceDict<'_, '_>>> {
        self.dict.type3()?.resources()
    }

    pub fn get_glyph(&self, gid: u16) -> Option<&Type3Glyph> {
        self.glyphs.get(gid as usize)
    }

    pub fn matrix(&self) -> Result<GlyphToTextSpace> {
        self.dict.type3()?.matrix()
    }
}

struct Type3FontOp<'a> {
    font_width: FirstLastFontWidth,
    encoding: Encoding,
    name_to_gid: &'a HashMap<Name, u16>,
    units_per_em: u16,
}

impl<'a> Type3FontOp<'a> {
    fn new(font_dict: &FontDict<'_, '_>, name_to_gid: &'a HashMap<Name, u16>) -> Result<Self> {
        let encoding = super::EncodingParser(font_dict).type3()?;
        let type3 = font_dict.type3()?;
        let matrix = type3.matrix()?;

        Ok(Self {
            font_width: FirstLastFontWidth::from(font_dict)?
                .whatever_context::<_, ObjectValueError>("Get FirstLastFontWidth")?,
            name_to_gid,
            encoding,
            units_per_em: (1.0 / matrix.m11)
                .abs()
                .to_u16()
                .whatever_context::<_, ObjectValueError>("units_per_em to u16")?,
        })
    }
}

impl FontOp for Type3FontOp<'_> {
    fn decode_chars(&self, s: &[u8]) -> Result<Vec<u32>> {
        Ok(s.iter().map(|v| *v as u32).collect())
    }

    fn char_to_gid(&self, ch: u32) -> Result<u16> {
        let gid_name = self.encoding.get_str(
            ch.try_into()
                .whatever_context::<_, ObjectValueError>("convert ch to u8")?,
        );
        if let Some(gid) = self.name_to_gid.get(gid_name) {
            Ok(*gid)
        } else {
            info!("glyph id not found for char: {:?}/{}", ch, gid_name);
            Ok(u16::MAX)
        }
    }

    fn char_advance(&self, ch: u32) -> Result<GlyphLength> {
        Ok(self.font_width.char_width(ch))
    }

    fn units_per_em(&self) -> Result<u16> {
        Ok(self.units_per_em)
    }
}

#[derive(Debug)]
pub struct Type3Glyph(Box<[Operation]>);

impl Type3Glyph {
    pub fn operations(&self) -> &[Operation] {
        &self.0
    }
}

impl<P: PathSink + 'static> Font<P> for Type3Font<'_, '_> {
    fn font_type(&self) -> super::FontType {
        super::FontType::Type3
    }

    fn create_op(&self, _cmap_registry: &mut super::CMapRegistry) -> Result<Box<dyn FontOp + '_>> {
        Ok(Box::new(Type3FontOp::new(&self.dict, &self.name_to_gid)?))
    }

    fn create_glyph_render(&self) -> Result<Box<dyn GlyphRender<P> + '_>> {
        struct StubGlyphRender;

        impl<P> GlyphRender<P> for StubGlyphRender {
            fn render(&self, _gid: u16, _sink: &mut P) {
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
