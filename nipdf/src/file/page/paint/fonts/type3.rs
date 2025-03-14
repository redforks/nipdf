use super::{
    Encoding, FirstLastFontWidth, Font, FontDict, GlyphRender, Operation, PathSink,
    parse_operations,
};
use crate::file::ObjectResolver;
use crate::file::page::paint::fonts::FontOp;
use crate::object::{RootPdfObject as _, RuntimeObjectId, Stream};
use crate::{
    ObjectValueError, Result, file::page::ResourceDict, graphics::trans::GlyphToTextSpace,
};
use ahash::{HashMap, HashMapExt};
use log::{debug, info};
use num_traits::ToPrimitive;
use once_cell::unsync::OnceCell;
use prescript::Name;
use snafu::{OptionExt, ResultExt};
use winnow::{Parser as _, combinator::terminated, token::rest};

pub struct Type3Font {
    name_to_gid: HashMap<Name, u16>,
    glyphs: Box<[OnceCell<Type3Glyph>]>,
    dict_id: RuntimeObjectId,
    char_procs: HashMap<Name, Stream>,
    matrix: GlyphToTextSpace,
    encoding: Encoding,
    first_last_width: Option<FirstLastFontWidth>,
}

impl Type3Font {
    pub fn new(dict: FontDict<'_, '_>) -> Result<Self> {
        let char_procs = dict.type3()?.char_procs()?;

        let mut glyphs = Vec::with_capacity(char_procs.len());
        let mut glyph_ids = HashMap::with_capacity(char_procs.len());

        for (name, _) in &char_procs {
            let gid = glyphs
                .len()
                .try_into()
                .whatever_context::<_, ObjectValueError>("glyphs length convert to u16")?;
            glyphs.push(OnceCell::new());
            glyph_ids.insert(name.clone(), gid);
        }

        let matrix = dict.type3()?.matrix()?;
        let encoding = super::EncodingParser(&dict).type3()?;
        let first_last_width = FirstLastFontWidth::from(&dict)?;

        Ok(Self {
            name_to_gid: glyph_ids,
            glyphs: glyphs.into(),
            dict_id: dict.id(),
            char_procs: char_procs
                .into_iter()
                .map(|(k, v)| (k, v.clone()))
                .collect(),
            matrix,
            encoding,
            first_last_width,
        })
    }

    pub fn resources<'b>(
        &self,
        resolver: &'b ObjectResolver<'_>,
    ) -> Result<Option<ResourceDict<'b, '_>>> {
        let dict: FontDict<'_, '_> = resolver.resolve_pdf_object(self.dict_id)?;
        dict.type3()?.resources()
    }

    pub fn get_glyph<'b>(&self, gid: u16, resolver: &ObjectResolver<'b>) -> Option<&Type3Glyph> {
        // Get the cell from the glyphs array
        let cell = self.glyphs.get(gid as usize)?;

        // If the cell is already initialized, return it directly
        if cell.get().is_some() {
            return cell.get();
        }

        // Find the name for this glyph ID
        let name = self
            .name_to_gid
            .iter()
            .find_map(|(name, &id)| if id == gid { Some(name) } else { None })?;

        // Get the stream for this glyph
        let stream = self.char_procs.get(name)?;

        // Parse the glyph on demand and cache it
        cell.get_or_try_init(|| {
            debug!("parse Type3 glyph: {}", name.as_str());
            let data = stream
                .decode(resolver)
                .whatever_context::<_, ObjectValueError>("decode stream")?;
            let ops = terminated(parse_operations::<crate::ParserError>, rest)
                .parse(&data[..])
                .map_err(winnow::error::ParseError::into_inner)
                .whatever_context::<_, ObjectValueError>("parse type3 operation")?;
            Ok::<_, ObjectValueError>(Type3Glyph(ops.into()))
        })
        .ok()
    }

    pub fn matrix(&self) -> GlyphToTextSpace {
        self.matrix
    }
}

struct Type3FontOp {
    encoding: Encoding,
    name_to_gid: HashMap<Name, u16>,
    units_per_em: u16,
}

impl Type3FontOp {
    fn new(
        name_to_gid: HashMap<Name, u16>,
        encoding: Encoding,
        matrix: GlyphToTextSpace,
    ) -> Result<Self> {
        Ok(Self {
            name_to_gid,
            encoding,
            units_per_em: (1.0 / matrix.m11)
                .abs()
                .to_u16()
                .whatever_context::<_, ObjectValueError>("units_per_em to u16")?,
        })
    }
}

impl FontOp for Type3FontOp {
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

impl<P: PathSink + 'static> Font<P> for Type3Font {
    fn create_op(&self) -> Result<Box<dyn FontOp>> {
        Ok(Box::new(Type3FontOp::new(
            self.name_to_gid.clone(),
            self.encoding.clone(),
            self.matrix,
        )?))
    }

    fn create_glyph_render(&self) -> Result<Box<dyn GlyphRender<P>>> {
        struct StubGlyphRender;

        impl<P> GlyphRender<P> for StubGlyphRender {
            fn render(&self, _gid: u16, _sink: &mut P) {
                // Paint::show_texts() do not use GlyphRender to render glyphs
                unreachable!()
            }
        }

        Ok(Box::new(StubGlyphRender))
    }

    fn as_type3(&self) -> Option<&Type3Font> {
        Some(self)
    }

    fn create_glyph_width(&self) -> Result<Box<dyn super::GlyphAdvance>> {
        Ok(Box::new(self.first_last_width.clone().unwrap()))
    }
}
