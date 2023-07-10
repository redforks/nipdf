use crate::object::Array;

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
    // pdf object id
    id: u32,
    media_box: Rectangle,
    crop_box: Rectangle,
}

#[cfg(test)]
mod tests;
