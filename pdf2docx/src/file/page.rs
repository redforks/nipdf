use nom::Finish;
use pdf2docx_macro::{pdf_object, TryFromNameObject};
use tiny_skia::Pixmap;

use crate::{
    graphics::{
        parse_operations, Color, LineCapStyle, LineJoinStyle, Operation, PatternDict, Point,
        RenderingIntent,
    },
    object::{Dictionary, FilterDecodedData, Object, ObjectValueError, PdfObject, Stream},
    text::FontDict,
};

use self::paint::Render;
pub use self::paint::{RenderOption, RenderOptionBuilder};

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

    pub fn left_lower(&self) -> Point {
        Point::new(self.left_x, self.lower_y)
    }

    pub fn right_upper(&self) -> Point {
        Point::new(self.right_x, self.upper_y)
    }
}

impl From<Rectangle> for tiny_skia::Rect {
    fn from(rect: Rectangle) -> Self {
        Self::from_ltrb(rect.left_x, rect.lower_y, rect.right_x, rect.upper_y).unwrap()
    }
}

/// Convert from raw array, auto re-order to (left_x, lower_y, right_x, upper_y),
/// see PDF 32000-1:2008 7.9.5
impl<'a> TryFrom<&Object<'a>> for Rectangle {
    type Error = ObjectValueError;
    fn try_from(object: &Object<'a>) -> Result<Self, Self::Error> {
        match object {
            Object::Array(arr) => {
                let mut iter = arr.iter();
                let left_x = iter.next().unwrap().as_number().unwrap();
                let lower_y = iter.next().unwrap().as_number().unwrap();
                let right_x = iter.next().unwrap().as_number().unwrap();
                let upper_y = iter.next().unwrap().as_number().unwrap();
                Ok(Self::from_lbrt(left_x, lower_y, right_x, upper_y))
            }
            _ => Err(ObjectValueError::GraphicsOperationSchemaError),
        }
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

#[derive(Copy, Clone, PartialEq, Eq, Debug, TryFromNameObject)]
pub enum XObjectType {
    Image,
    Form,
    /// PostScript XObject
    PS,
}

#[pdf_object(Some("XObject"))]
pub trait XObjectDictTrait {
    #[try_from]
    fn subtype(&self) -> Option<XObjectType>;

    // available if it is soft-mask image, see Table 146
    #[try_from]
    fn matte(&self) -> Option<Color>;

    #[nested]
    fn s_mask(&self) -> Option<XObjectDict<'a, 'b>>;
}

impl<'a, 'b> XObjectDict<'a, 'b> {
    fn as_image(&self) -> Option<&Stream<'a>> {
        if self.subtype().unwrap() == Some(XObjectType::Image) {
            Some(
                self.d
                    .resolver()
                    .resolve(self.id().unwrap())
                    .unwrap()
                    .as_stream()
                    .unwrap(),
            )
        } else {
            None
        }
    }
}

#[pdf_object(())]
pub trait ResourceDictTrait {
    #[nested]
    fn ext_g_state() -> HashMap<String, GraphicsStateParameterDict<'a, 'b>>;
    fn color_space(&self) -> Option<&'b Dictionary<'a>>;
    #[nested]
    fn pattern(&self) -> HashMap<String, PatternDict<'a, 'b>>;
    fn shading(&self) -> Option<&'b Dictionary<'a>>;
    #[nested]
    fn x_object(&self) -> HashMap<String, XObjectDict<'a, 'b>>;
    #[nested]
    fn font(&self) -> HashMap<String, FontDict<'a, 'b>>;
    fn properties(&self) -> Option<&'b Dictionary<'a>>;
}

#[pdf_object(["Pages", "Page"])]
pub(crate) trait PageDictTrait {
    #[nested]
    fn kids(&self) -> Vec<Self>;
    #[try_from]
    fn media_box(&self) -> Option<Rectangle>;
    #[try_from]
    fn crop_box(&self) -> Option<Rectangle>;
    #[nested]
    fn resources(&self) -> Option<ResourceDict<'a, 'b>>;
    fn contents(&self) -> Vec<&Stream<'a>>;
    #[key("Type")]
    #[typ("Name")]
    fn type_name(&self) -> &str;
}

impl<'a, 'b> PageDict<'a, 'b> {
    pub fn is_leaf(&self) -> bool {
        self.type_name().unwrap() == "Page"
    }
}

#[derive(Debug)]
pub struct Page<'a, 'b> {
    d: PageDict<'a, 'b>,
    parents_to_root: Vec<PageDict<'a, 'b>>,
}

impl<'a, 'b> Page<'a, 'b> {
    pub fn id(&self) -> u32 {
        self.d.id().unwrap().get()
    }

    fn iter_to_root(&self) -> impl Iterator<Item = &PageDict<'a, 'b>> {
        once(&self.d).chain(self.parents_to_root.iter())
    }

    pub fn media_box(&self) -> Rectangle {
        self.iter_to_root()
            .find_map(|d| d.media_box().unwrap())
            .expect("page must have media box")
    }

    pub fn crop_box(&self) -> Option<Rectangle> {
        self.iter_to_root().find_map(|d| d.crop_box().unwrap())
    }

    fn resources(&self) -> ResourceDict<'a, 'b> {
        self.iter_to_root()
            .find_map(|d| d.resources().unwrap())
            .expect("page must have resources")
    }

    pub fn content(&self) -> Result<PageContent, ObjectValueError> {
        let bufs = self
            .d
            .contents()
            .unwrap()
            .into_iter()
            .map(|s| {
                let decoded = s.decode(self.d.d.resolver(), false)?;
                match decoded {
                    FilterDecodedData::Bytes(b) => Ok::<_, ObjectValueError>(b.into_owned()),
                    _ => {
                        panic!("expected page content is stream");
                    }
                }
            })
            .collect::<Result<_, _>>()?;
        Ok(PageContent { bufs })
    }

    pub fn render_steps(
        &self,
        option: RenderOptionBuilder,
        steps: Option<usize>,
    ) -> Result<Pixmap, ObjectValueError> {
        let media_box = self.media_box();
        let option = option
            .width(media_box.width() as u32)
            .height(media_box.height() as u32)
            .build();
        let content = self.content()?;
        let ops = content.operations();
        let resource = self.resources();
        let mut renderer = Render::new(option, &resource);
        if let Some(steps) = steps {
            for op in ops.into_iter().take(steps) {
                renderer.exec(&op);
            }
        } else {
            for op in ops.into_iter() {
                renderer.exec(&op);
            }
        };
        Ok(renderer.into())
    }

    pub fn render(&self, option: RenderOptionBuilder) -> Result<Pixmap, ObjectValueError> {
        self.render_steps(option, None)
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
                let kids = node.kids().unwrap();
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
    pub fn operations(&self) -> Vec<Operation<'_>> {
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
