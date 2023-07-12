use crate::object::{Array, Dictionary, ObjectValueError, SchemaDict};

use super::ObjectResolver;
use std::iter::once;

#[derive(Debug, Copy, Clone)]
pub struct Rectangle {
    pub left_x: f32,
    pub lower_y: f32,
    pub right_x: f32,
    pub upper_y: f32,
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

struct PageDict<'a, 'b> {
    d: SchemaDict<'a, 'b, [&'static str; 2]>,
}

impl<'a, 'b> PageDict<'a, 'b> {
    pub fn new(id: u32, dict: &'b Dictionary<'a>) -> Result<Self, ObjectValueError> {
        let d = SchemaDict::new(id, dict, ["Pages", "Page"])?;
        Ok(Self { d })
    }

    pub fn is_leaf(&self) -> bool {
        self.d.type_name() == "Page"
    }

    pub fn parent_id(&self) -> Option<u32> {
        self.d.opt_int("Parent").unwrap().map(|id| id as u32)
    }

    pub fn kids(&self) -> Vec<u32> {
        self.d
            .opt_arr_map("Kids", |o| Ok(o.as_ref()?.id().id()))
            .unwrap()
            .unwrap_or_default()
    }

    pub fn media_box(&self) -> Option<Rectangle> {
        self.d.opt_rectangle("MediaBox").unwrap()
    }

    pub fn crop_box(&self) -> Option<Rectangle> {
        self.d.opt_rectangle("CropBox").unwrap()
    }
}

#[derive(Debug)]
pub struct Page {
    /// pdf object id
    id: u32,
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

    /// Parse page tree to get all pages
    pub fn parse<'a, 'b>(
        root_id: u32,
        resolver: &'b ObjectResolver<'a>,
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
            let d = PageDict::new(id, d.as_dict()?)?;
            if d.is_leaf() {
                pages.push(Page::from_leaf(id, &d, &parents[..]));
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

    fn from_leaf<'a, 'b>(id: u32, d: &PageDict<'a, 'b>, parents: &[PageDict<'a, 'b>]) -> Self {
        let media_box = once(d)
            .chain(parents.iter())
            .map(|d| d.media_box())
            .find_map(|r| r)
            .unwrap();
        let crop_box = once(d)
            .chain(parents.iter())
            .map(|d| d.crop_box())
            .find_map(|r| r);

        Self {
            id,
            media_box,
            crop_box,
        }
    }
}

#[cfg(test)]
mod tests;
