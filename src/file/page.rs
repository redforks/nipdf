use crate::object::{Array, Dictionary, ObjectValueError, SchemaDict};


use super::ObjectResolver;

#[derive(Debug, Copy, Clone)]
pub struct Rectangle {
    pub left_x: f32,
    pub lower_y: f32,
    pub right_x: f32,
    pub upper_y: f32,
}

/// Convert from raw array, auto re-order to (left_x, lower_y, right_x, upper_y),
/// see PDF 32000-1:2008 7.9.5
impl<'a> From<Array<'a>> for Rectangle {
    fn from(arr: Array<'a>) -> Self {
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

pub struct Page {
    /// pdf object id
    id: u32,
    media_box: Rectangle,
    crop_box: Option<Rectangle>,
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
            .opt_arr("Kids", |o| Ok(o.as_int()? as u32))
            .unwrap()
            .unwrap_or_default()
    }
}

impl Page {
    /// Parse page tree to get all pages
    pub fn parse(
        root_id: u32,
        resolver: &mut ObjectResolver,
    ) -> Result<Vec<Page>, ObjectValueError> {
        let mut pages = Vec::new();
        let mut stack = vec![root_id];
        while let Some(id) = stack.pop() {
            let d = resolver.resolve(id)?;
            let d = PageDict::new(root_id, d.as_dict()?)?;
            if d.is_leaf() {
                pages.push(Page::from_leaf(id, &d));
            } else {
                stack.extend(d.kids());
            }
        }
        pages.reverse();
        Ok(pages)
    }

    fn from_leaf(id: u32, _d: &PageDict) -> Self {
        Self {
            id,
            media_box: Rectangle {
                left_x: 0.0,
                lower_y: 0.0,
                right_x: 0.0,
                upper_y: 0.0,
            },
            crop_box: None,
        }
    }
}

#[cfg(test)]
mod tests;
