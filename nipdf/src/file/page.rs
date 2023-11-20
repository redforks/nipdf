use self::paint::Render;
pub use self::paint::{RenderOption, RenderOptionBuilder};
use crate::{
    graphics::{
        parse_operations, shading::ShadingDict, trans::FormToUserSpace, ColorArgs, ColorSpaceArgs,
        LineCapStyle, LineJoinStyle, Operation, PatternDict, Point, RenderingIntent,
    },
    object::{Dictionary, Object, ObjectValueError, PdfObject, Stream},
    text::FontDict,
};
use ahash::{HashMap, HashMapExt};
use educe::Educe;
use log::error;
use nipdf_macro::{pdf_object, TryFromNameObject};
use nom::Finish;
use prescript::Name;
use prescript_macro::name;
use std::iter::once;
use tiny_skia::Pixmap;

mod paint;

#[derive(Debug, Copy, Clone, PartialEq)]
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
impl TryFrom<&Object> for Rectangle {
    type Error = ObjectValueError;

    fn try_from(object: &Object) -> Result<Self, Self::Error> {
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
    #[key("FL")]
    fn flatness(&self) -> Option<f32>;
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
    fn subtype(&self) -> XObjectType;

    // available if it is soft-mask image, see Table 146
    #[try_from]
    fn matte(&self) -> Option<ColorArgs>;

    #[nested]
    fn s_mask(&self) -> Option<XObjectDict<'a, 'b>>;

    #[or_default]
    fn interpolate(&self) -> bool;

    #[or_default]
    fn image_mask(&self) -> bool;

    #[self_as]
    fn as_form(&self) -> FormXObjectDict<'a, 'b>;
}

impl<'a, 'b> XObjectDict<'a, 'b> {
    fn as_stream(&self) -> Result<&Stream, ObjectValueError> {
        self.d.resolver().resolve(self.id().unwrap())?.as_stream()
    }
}

#[pdf_object((Some("XObject"), "Form"))]
pub trait FormXObjectDictTrait {
    #[try_from]
    fn b_box(&self) -> Rectangle;

    #[try_from]
    #[or_default]
    fn matrix(&self) -> FormToUserSpace;

    #[nested]
    fn resources(&self) -> Option<ResourceDict<'a, 'b>>;

    fn group(&self) -> Option<&'b Dictionary>;

    #[key("Ref")]
    fn ref_dict(&self) -> Option<&'b Dictionary>;

    fn metadata(&self) -> Option<&'b Dictionary>;

    fn piece_info(&self) -> Option<&'b Dictionary>;
}

/// Wrap type to impl TryFrom<> trait
#[derive(Educe)]
#[educe(Deref)]
pub struct ColorSpaceResources(HashMap<Name, ColorSpaceArgs>);

impl<'b> TryFrom<&'b Object> for ColorSpaceResources {
    type Error = ObjectValueError;

    fn try_from(object: &'b Object) -> Result<Self, Self::Error> {
        let mut map = HashMap::new();
        match object {
            Object::Dictionary(dict) => {
                for (k, v) in dict.iter() {
                    let cs = ColorSpaceArgs::try_from(v)?;
                    map.insert(k.clone(), cs);
                }
                Ok(Self(map))
            }
            _ => {
                error!("{:?}", object);
                Err(ObjectValueError::GraphicsOperationSchemaError)
            }
        }
    }
}

#[pdf_object(())]
pub trait ResourceDictTrait {
    #[nested]
    fn ext_g_state(&self) -> HashMap<Name, GraphicsStateParameterDict<'a, 'b>>;
    #[try_from]
    fn color_space(&self) -> ColorSpaceResources;
    #[nested]
    fn pattern(&self) -> HashMap<Name, PatternDict<'a, 'b>>;
    #[nested]
    fn shading(&self) -> HashMap<Name, ShadingDict<'a, 'b>>;
    #[nested]
    fn x_object(&self) -> HashMap<Name, XObjectDict<'a, 'b>>;
    #[nested]
    fn font(&self) -> HashMap<Name, FontDict<'a, 'b>>;
    fn properties(&self) -> Option<&'b Dictionary>;
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
    fn contents(&self) -> Vec<&Stream>;
    #[key("Type")]
    fn type_name(&self) -> &Name;
}

impl<'a, 'b> PageDict<'a, 'b> {
    pub fn is_leaf(&self) -> bool {
        self.type_name().unwrap() == &name!("Page")
    }
}

#[derive(Debug)]
pub struct Page<'a, 'b> {
    d: PageDict<'a, 'b>,
    parents_to_root: Vec<PageDict<'a, 'b>>,
}

impl<'a, 'b: 'a> Page<'a, 'b> {
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
            .map(|s| s.decode(self.d.d.resolver()).map(|v| v.into_owned()))
            .collect::<Result<_, _>>()?;
        Ok(PageContent { bufs })
    }

    pub fn render_steps(
        &self,
        option: RenderOptionBuilder,
        steps: Option<usize>,
    ) -> Result<Pixmap, ObjectValueError> {
        let media_box = self.media_box();
        let crop_box = self.crop_box();
        let option = option
            .width(media_box.width() as u32)
            .height(media_box.height() as u32)
            .crop(need_crop(crop_box, media_box).then(|| crop_box.unwrap()))
            .build();
        let content = self.content()?;
        let ops = content.operations();
        let resource = self.resources();
        let mut canvas = option.create_canvas();
        let mut renderer = Render::new(&mut canvas, option, &resource);
        if let Some(steps) = steps {
            ops.into_iter().take(steps).for_each(|op| renderer.exec(op));
        } else {
            ops.into_iter().for_each(|op| renderer.exec(op));
        };
        drop(renderer);
        Ok(canvas)
    }

    pub fn render(&self, option: RenderOptionBuilder) -> Result<Pixmap, ObjectValueError> {
        self.render_steps(option, None)
    }

    /// Parse page tree to get all pages
    pub(crate) fn parse(root: PageDict<'a, 'b>) -> Result<Vec<Self>, ObjectValueError> {
        let mut pages = Vec::new();
        let mut parents = Vec::new();
        fn handle<'a, 'b: 'a, 'c>(
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

fn need_crop(crop: Option<Rectangle>, media: Rectangle) -> bool {
    match crop {
        None => false,
        Some(crop) => crop != media,
    }
}

pub struct PageContent {
    bufs: Vec<Vec<u8>>,
}

impl PageContent {
    pub fn new(bufs: Vec<Vec<u8>>) -> Self {
        Self { bufs }
    }

    pub fn operations(&self) -> Vec<Operation> {
        let mut r = vec![];
        for buf in &self.bufs {
            let (input, ops) = parse_operations(buf.as_ref()).finish().unwrap();
            assert!(input.is_empty(), "buf should be empty: {:?}", input);
            r.extend_from_slice(ops.as_slice());
        }
        r
    }

    pub fn as_ref(&self) -> impl Iterator<Item = &[u8]> {
        self.bufs.iter().map(|v| v.as_ref())
    }
}

#[cfg(test)]
mod tests;
