use crate::object::Object;
use test_case::test_case;

use super::*;

#[test_case(1.0, 2, 3.0, 4.0 => (1.0, 2.0, 3.0, 4.0); "normal")]
#[test_case(3.0, 4, 1.0, 2.0 => (1.0, 2.0, 3.0, 4.0); "auto reorder")]
fn rectangle_from_array(
    x1: impl Into<Object<'static>>,
    y1: impl Into<Object<'static>>,
    x2: impl Into<Object<'static>>,
    y2: impl Into<Object<'static>>,
) -> (f32, f32, f32, f32) {
    let arr = Array::from(vec![x1.into(), y1.into(), x2.into(), y2.into()]);
    let rect = Rectangle::from(arr);
    (rect.left_x, rect.lower_y, rect.right_x, rect.upper_y)
}
