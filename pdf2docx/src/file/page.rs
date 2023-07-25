use nom::Finish;
use pdf2docx_macro::pdf_object;
use tiny_skia::Pixmap;

use crate::{
    graphics::{parse_operations, LineCapStyle, LineJoinStyle, Operation, RenderingIntent},
    object::{Array, Dictionary, FilterDecodedData, ObjectValueError, SchemaDict},
};

use super::ObjectResolver;
use std::iter::once;

mod paint;

#[derive(Debug, Copy, Clone)]
pub struct Rectangle {
    pub left_x: f32,
    pub lower_y: f32,
    pub right_x: f32,
    pub upper_y: f32,
}

impl Rectangle {
    pub fn width(&self) -> f32 {
        self.right_x - self.left_x
    }

    pub fn height(&self) -> f32 {
        self.upper_y - self.lower_y
    }
}

/// Convert from raw array, auto re-order to (left_x, lower_y, right_x, upper_y),
/// see PDF 32000-1:2008 7.9.5
impl<'a> From<&Array<'a>> for Rectangle {
    fn from(arr: &Array<'a>) -> Self {
        let mut iter = arr.iter();
        let left_x = iter.next().unwrap().as_number().unwrap();
        let lower_y = iter.next().unwrap().as_number().unwrap();
        let right_x = iter.next().unwrap().as_number().unwrap();
        let upper_y = iter.next().unwrap().as_number().unwrap();
        Self {
            left_x: left_x.min(right_x),
            lower_y: lower_y.min(upper_y),
            right_x: left_x.max(right_x),
            upper_y: lower_y.max(upper_y),
        }
    }
}

#[pdf_object(Some("ExtGState"))]
trait GraphicsStateParameterDictTrait {
    #[key("LW")]
    fn line_width(&self) -> Option<f32>;

    #[key("LC")]
    #[try_from]
    fn line_cap(&self) -> Option<LineCapStyle>;

    #[key("LJ")]
    #[try_from]
    fn line_join(&self) -> Option<LineJoinStyle>;

    #[key("ML")]
    fn miter_limit(&self) -> Option<f32>;

    #[key("RI")]
    #[try_from]
    fn rendering_intent(&self) -> Option<RenderingIntent>;

    #[key("SA")]
    fn stroke_adjustment(&self) -> Option<bool>;

    #[key("CA")]
    fn stroke_alpha(&self) -> Option<f32>;
    #[key("ca")]
    fn fill_alpha(&self) -> Option<f32>;
    #[key("AIS")]
    fn alpha_source_flag(&self) -> Option<bool>;
    #[key("TK")]
    fn text_knockout_flag(&self) -> Option<bool>;
}

impl<'a, 'b> GraphicsStateParameterDict<'a, 'b> {
    fn dash_pattern(&self) -> Option<(Vec<f32>, f32)> {
        self.d.opt_arr("D").unwrap().map(|arr| {
            let mut iter = arr.iter();
            let dash_array = iter.next().unwrap().as_arr().unwrap();
            let dash_phase = iter.next().unwrap().as_number().unwrap();
            let dash_array = dash_array.iter().map(|o| o.as_number().unwrap()).collect();
            (dash_array, dash_phase)
        })
    }
}

#[pdf_object(())]
trait ResourceDictTrait {
    #[nested]
    fn ext_g_state() -> Option<GraphicsStateParameterDict<'a, 'b>>;
    fn color_space(&self) -> Option<&'b Dictionary<'a>>;
    fn pattern(&self) -> Option<&'b Dictionary<'a>>;
    fn shading(&self) -> Option<&'b Dictionary<'a>>;
    fn x_object(&self) -> Option<&'b Dictionary<'a>>;
    fn font(&self) -> Option<&'b Dictionary<'a>>;
    fn properties(&self) -> Option<&'b Dictionary<'a>>;
}

#[pdf_object(["Pages", "Page"])]
trait PageDictTrait {
    #[typ("Ref")]
    fn kids(&self) -> Vec<u32>;
    fn media_box(&self) -> Option<Rectangle>;
    fn crop_box(&self) -> Option<Rectangle>;
    #[nested]
    fn resources(&self) -> Option<ResourceDict<'a, 'b>>;
}

impl<'a, 'b> PageDict<'a, 'b> {
    pub fn is_leaf(&self) -> bool {
        self.d.type_name() == "Page"
    }
}

#[derive(Debug)]
pub struct Page {
    /// pdf object id
    id: u32,
    content_ids: Vec<u32>, // maybe empty
    media_box: Rectangle,
    crop_box: Option<Rectangle>,
}

impl Page {
    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn media_box(&self) -> Rectangle {
        self.media_box
    }

    pub fn crop_box(&self) -> Option<Rectangle> {
        self.crop_box
    }

    pub fn content(&self, resolver: &ObjectResolver<'_>) -> Result<PageContent, ObjectValueError> {
        let mut bufs = Vec::with_capacity(self.content_ids.len());
        for id in &self.content_ids {
            let s = resolver.resolve(*id)?.as_stream()?;
            let decoded = s.decode(false)?;
            match decoded {
                FilterDecodedData::Bytes(b) => bufs.push(b.into_owned()),
                _ => {
                    panic!("expected page content is stream");
                }
            }
        }
        Ok(PageContent { bufs })
    }

    pub fn render(&self, resolver: &ObjectResolver<'_>) -> Result<Pixmap, ObjectValueError> {
        let media_box = self.media_box();
        let map = Pixmap::new(media_box.width() as u32, media_box.height() as u32).unwrap();
        let mut renderer = paint::Render::new(map);
        let content = self.content(resolver)?;
        for op in content.operations() {
            renderer.exec(&op);
        }
        Ok(renderer.into())
    }

    /// Parse page tree to get all pages
    pub(crate) fn parse(
        root_id: u32,
        resolver: &ObjectResolver<'_>,
    ) -> Result<Vec<Page>, ObjectValueError> {
        let mut pages = Vec::new();
        let mut parents = Vec::new();
        fn handle<'a, 'b, 'c>(
            id: u32,
            resolver: &'b ObjectResolver<'a>,
            pages: &'c mut Vec<Page>,
            parents: &'c mut Vec<PageDict<'a, 'b>>,
        ) -> Result<(), ObjectValueError> {
            let d = resolver.resolve(id).unwrap();
            let d = PageDict::new(d.as_dict()?)?;
            if d.is_leaf() {
                pages.push(Page::from_leaf(id, &d, &parents[..])?);
            } else {
                let kids = d.kids();
                parents.push(d);
                for kid in kids {
                    handle(kid, resolver, pages, parents)?;
                }
            }
            Ok(())
        }
        handle(root_id, resolver, &mut pages, &mut parents)?;
        Ok(pages)
    }

    fn from_leaf<'a, 'b>(
        id: u32,
        d: &PageDict<'a, 'b>,
        parents: &[PageDict<'a, 'b>],
    ) -> Result<Self, ObjectValueError> {
        let media_box = once(d)
            .chain(parents.iter())
            .map(|d| d.media_box())
            .find_map(|r| r)
            .ok_or(ObjectValueError::DictSchemaError("Page", "MediaBox"))?;
        let crop_box = once(d)
            .chain(parents.iter())
            .map(|d| d.crop_box())
            .find_map(|r| r);
        let content_ids =
            d.d.opt_single_or_arr("Contents", |o| Ok(o.as_ref()?.id().id()))?;

        Ok(Self {
            id,
            media_box,
            crop_box,
            content_ids,
        })
    }
}

pub struct PageContent {
    bufs: Vec<Vec<u8>>,
}

impl PageContent {
    pub fn operations(&self) -> Vec<Operation> {
        let mut r = vec![];
        for buf in &self.bufs {
            let (input, ops) = parse_operations(buf.as_ref()).finish().unwrap();
            assert!(input.is_empty(), "buf should be empty: {:?}", input);
            r.extend_from_slice(ops.as_slice());
        }
        r
    }
}

#[cfg(test)]
mod tests;
