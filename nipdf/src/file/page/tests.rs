use super::*;
use crate::{
    file::{ObjectResolver, XRefTable},
    object::{Array, Object},
};
use test_case::test_case;

#[test_case(1.0, 2, 3.0, 4.0 => (1.0, 2.0, 3.0, 4.0); "normal")]
#[test_case(3.0, 4, 1.0, 2.0 => (1.0, 2.0, 3.0, 4.0); "auto reorder")]
fn rectangle_from_array(
    x1: impl Into<Object>,
    y1: impl Into<Object>,
    x2: impl Into<Object>,
    y2: impl Into<Object>,
) -> (f32, f32, f32, f32) {
    let arr = vec![x1.into(), y1.into(), x2.into(), y2.into()].into();
    let rect = Rectangle::try_from(&arr).unwrap();
    (rect.left_x, rect.lower_y, rect.right_x, rect.upper_y)
}

#[test_case(1, vec![(1, vec![2]), (2, vec![])]=> vec![2u32]; "one page")]
#[test_case(1, vec![
    (1, vec![2, 3, 4]),
    (2, vec![]),
    (3, vec![5, 6]),
    (4, vec![7, 8]),
    (5, vec![]),
    (6, vec![]),
    (7, vec![9]),
    (8, vec![]),
    (9, vec![]),
]=> vec![2, 5, 6, 9, 8]; "complex tree")]
fn parse_page_tree(root_id: u32, tree: Vec<(u32, Vec<u32>)>) -> Vec<u32> {
    let xref = XRefTable::empty();
    let mut resolver = ObjectResolver::empty(&xref);
    for (id, kids) in tree {
        let mut dict = HashMap::new();
        dict.insert(
            sname("Type"),
            (if kids.is_empty() { "/Page" } else { "/Pages" }).into(),
        );
        dict.insert(
            sname("MediaBox"),
            Object::Array(vec![0.0.into(), 0.0.into(), 0.0.into(), 0.0.into()].into()),
        );
        dict.insert(
            sname("Kids"),
            Object::Array(kids.into_iter().map(Object::new_ref).collect::<Array>()),
        );
        resolver.setup_object(id, Object::Dictionary(Dictionary::from(dict)));
    }

    let pages = Page::parse(resolver.resolve_pdf_object(root_id).unwrap());
    pages.unwrap().into_iter().map(|p| p.id().0).collect()
}
