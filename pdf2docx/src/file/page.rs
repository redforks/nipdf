use nom::Finish;
use pdf2docx_macro::pdf_object;
use tiny_skia::Pixmap;

use crate::{
    graphics::{parse_operations, LineCapStyle, LineJoinStyle, Operation, RenderingIntent},
    object::{
        Array, Dictionary, FilterDecodedData, ObjectValueError, PdfObject, RootPdfObject,
        SchemaDict,
    },
};

use super::ObjectResolver;
use std::{collections::HashMap, iter::once};

mod paint;

#[derive(Debug, Copy, Clone)]
pub struct Rectangle {
    pub left_x: f32,
    pub lower_y: f32,
    pub right_x: f32,
    pub upper_y: f32,
}

impl Rectangle {
    /// From left, top, right, bottom, re-order them to make sure that
    /// left <= right, top <= bottom
    pub fn from_lbrt(left_x: f32, bottom_y: f32, right_x: f32, top_y: f32) -> Self {
        Self {
            left_x: left_x.min(right_x),
            lower_y: bottom_y.min(top_y),
            right_x: left_x.max(right_x),
            upper_y: bottom_y.max(top_y),
        }
    }

    pub fn from_xywh(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self::from_lbrt(x, y, x + w, y + h)
    }

    pub fn width(&self) -> f32 {
        self.right_x - self.left_x
    }

    pub fn height(&self) -> f32 {
        self.upper_y - self.lower_y
    }
}

impl From<Rectangle> for tiny_skia::Rect {
    fn from(rect: Rectangle) -> Self {
        Self::from_ltrb(rect.left_x, rect.lower_y, rect.right_x, rect.upper_y).unwrap()
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
        Self::from_lbrt(left_x, lower_y, right_x, upper_y)
    }
}

#[pdf_object(Some("ExtGState"))]
pub trait GraphicsStateParameterDictTrait {
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
pub trait ResourceDictTrait {
    #[nested]
    fn ext_g_state() -> HashMap<String, GraphicsStateParameterDict<'a, 'b>>;
    fn color_space(&self) -> Option<&'b Dictionary<'a>>;
    fn pattern(&self) -> Option<&'b Dictionary<'a>>;
    fn shading(&self) -> Option<&'b Dictionary<'a>>;
    fn x_object(&self) -> Option<&'b Dictionary<'a>>;
    fn font(&self) -> Option<&'b Dictionary<'a>>;
    fn properties(&self) -> Option<&'b Dictionary<'a>>;
}

#[pdf_object(["Pages", "Page"])]
#[root_object]
pub trait PageDictTrait {
    #[nested_root]
    fn kids(&self) -> Vec<Self>;
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
pub struct Page<'a, 'b> {
    d: PageDict<'a, 'b>,
    parents_to_root: Vec<PageDict<'a, 'b>>,
}

impl<'a, 'b> Page<'a, 'b> {
    pub fn id(&self) -> u32 {
        self.d.id()
    }

    fn iter_to_root(&self) -> impl Iterator<Item = &PageDict<'a, 'b>> {
        once(&self.d).chain(self.parents_to_root.iter())
    }

    pub fn media_box(&self) -> Rectangle {
        self.iter_to_root()
            .find_map(|d| d.media_box())
            .expect("page must have media box")
    }

    pub fn crop_box(&self) -> Option<Rectangle> {
        self.iter_to_root().find_map(|d| d.crop_box())
    }

    pub fn content(&self, resolver: &ObjectResolver<'_>) -> Result<PageContent, ObjectValueError> {
        let content_ids = self
            .d
            .d
            .opt_single_or_arr("Contents", |o| Ok(o.as_ref()?.id().id()))?;

        let mut bufs = Vec::with_capacity(content_ids.len());
        for id in &content_ids[..] {
            let s = resolver.resolve(*id)?.as_stream()?;
            let decoded = s.decode(resolver, false)?;
            match decoded {
                FilterDecodedData::Bytes(b) => bufs.push(b.into_owned()),
                _ => {
                    panic!("expected page content is stream");
                }
            }
        }
        Ok(PageContent { bufs })
    }

    pub fn render(&self, resolver: &'b ObjectResolver<'a>) -> Result<Pixmap, ObjectValueError> {
        let media_box = self.media_box();
        let map = Pixmap::new(media_box.width() as u32, media_box.height() as u32).unwrap();
        let mut renderer = paint::Render::new(map, media_box.height() as u32);
        let content = self.content(resolver)?;
        let empty_dict = Dictionary::new();
        let resource = self
            .d
            .resources()
            .unwrap_or_else(|| ResourceDict::new(&empty_dict, resolver).unwrap());
        for op in content.operations() {
            renderer.exec(&op, &resource);
        }
        Ok(renderer.into())
    }

    /// Parse page tree to get all pages
    pub(crate) fn parse(root: PageDict<'a, 'b>) -> Result<Vec<Self>, ObjectValueError> {
        let mut pages = Vec::new();
        let mut parents = Vec::new();
        fn handle<'a, 'b, 'c>(
            node: PageDict<'a, 'b>,
            pages: &'c mut Vec<Page<'a, 'b>>,
            parents: &'c mut Vec<PageDict<'a, 'b>>,
        ) -> Result<(), ObjectValueError> {
            if node.is_leaf() {
                pages.push(Page::from_leaf(&node, &parents[..])?);
            } else {
                let kids = node.kids();
                parents.push(node);
                for kid in kids {
                    handle(kid, pages, parents)?;
                }
            }
            Ok(())
        }
        handle(root, &mut pages, &mut parents)?;
        Ok(pages)
    }

    fn from_leaf(
        d: &PageDict<'a, 'b>,
        parents: &[PageDict<'a, 'b>],
    ) -> Result<Self, ObjectValueError> {
        let mut parents = parents.to_vec();
        parents.reverse();

        Ok(Self {
            d: d.clone(),
            parents_to_root: parents,
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
