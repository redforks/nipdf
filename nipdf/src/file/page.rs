use crate::{
    function::Domains,
    graphics::{
        parse_operations, shading::ShadingDict, trans::FormToUserSpace, ColorArgs, ColorSpaceArgs,
        LineCapStyle, LineJoinStyle, Operation, PatternDict, Point, RenderingIntent,
    },
    object::{Dictionary, ImageMask, Object, ObjectValueError, PdfObject, RuntimeObjectId, Stream},
    text::FontDict,
};
use ahash::{HashMap, HashMapExt};
use educe::Educe;
use log::error;
use nipdf_macro::{pdf_object, TryFromNameObject};
use nom::Finish;
use prescript::{sname, Name};
use std::iter::once;

pub mod paint;

#[derive(Debug, Copy, Clone, PartialEq, Default)]
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

    pub fn zoom(&self, v: f32) -> Self {
        Self::from_lbrt(
            self.left_x * v,
            self.lower_y * v,
            self.right_x * v,
            self.upper_y * v,
        )
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
    fn alpha_is_shape(&self) -> Option<bool>;
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

    #[try_from]
    fn mask(&self) -> Option<ImageMask>;

    #[or_default]
    fn interpolate(&self) -> bool;

    #[or_default]
    fn image_mask(&self) -> bool;

    #[self_as]
    fn as_form(&self) -> FormXObjectDict<'a, 'b>;

    #[try_from]
    fn decode(&self) -> Option<Domains>;
}

impl<'a, 'b> XObjectDict<'a, 'b> {
    pub fn as_stream(&self) -> Result<&Stream, ObjectValueError> {
        self.d.resolver().resolve(self.id().unwrap())?.stream()
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
    #[one_or_more]
    fn contents(&self) -> Vec<&Stream>;
    #[key("Type")]
    fn type_name(&self) -> Name;
    #[or_default]
    fn rotate(&self) -> i32;
}

impl<'a, 'b> PageDict<'a, 'b> {
    pub fn is_leaf(&self) -> bool {
        self.type_name().unwrap() == sname("Page")
    }
}

#[derive(Debug)]
pub struct Page<'a, 'b> {
    d: PageDict<'a, 'b>,
    parents_to_root: Vec<PageDict<'a, 'b>>,
}

impl<'a, 'b: 'a> Page<'a, 'b> {
    pub fn id(&self) -> RuntimeObjectId {
        self.d.id().unwrap()
    }

    fn iter_to_root(&self) -> impl Iterator<Item = &PageDict<'a, 'b>> {
        once(&self.d).chain(self.parents_to_root.iter())
    }

    pub fn media_box(&self) -> Rectangle {
        self.iter_to_root()
            .find_map(|d| d.media_box().unwrap())
            .expect("page must have media box")
    }

    pub fn rotate(&self) -> i32 {
        self.d.rotate().unwrap()
    }

    /// Return None if crop_box not exist, or empty.
    pub fn crop_box(&self) -> Option<Rectangle> {
        let r = self.iter_to_root().find_map(|d| d.crop_box().unwrap());
        if let Some(r) = r {
            if r.width() == 0.0 || r.height() == 0.0 {
                return None;
            }
        }
        r
    }

    pub fn resources(&self) -> ResourceDict<'a, 'b> {
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

pub struct PageContent {
    bufs: Vec<Vec<u8>>,
}

impl PageContent {
    pub fn new(bufs: Vec<Vec<u8>>) -> Self {
        Self { bufs }
    }

    pub fn operations(self) -> Vec<Operation> {
        let mut data: Option<Vec<u8>> = None;
        for buf in self.bufs.into_iter() {
            if let Some(data) = data.as_mut() {
                data.extend_from_slice(&buf);
            } else {
                data = Some(buf);
            }
        }

        if let Some(data) = data {
            let (input, ops) = parse_operations(&data).finish().unwrap();
            assert!(input.is_empty(), "buf should be empty: {:?}", input);
            ops
        } else {
            vec![]
        }
    }

    pub fn as_ref(&self) -> impl Iterator<Item = &[u8]> {
        self.bufs.iter().map(|v| v.as_ref())
    }
}

#[cfg(test)]
mod tests;
