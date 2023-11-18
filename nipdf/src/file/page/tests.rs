use super::*;
use crate::{
    file::{ObjectResolver, XRefTable},
    object::{Array, Object},
};
use std::num::NonZeroU32;
use test_case::test_case;

#[test_case(1.0, 2, 3.0, 4.0 => (1.0, 2.0, 3.0, 4.0); "normal")]
#[test_case(3.0, 4, 1.0, 2.0 => (1.0, 2.0, 3.0, 4.0); "auto reorder")]
fn rectangle_from_array(
    x1: impl Into<Object<'static>>,
    y1: impl Into<Object<'static>>,
    x2: impl Into<Object<'static>>,
    y2: impl Into<Object<'static>>,
) -> (f32, f32, f32, f32) {
    let arr = Object::Array(vec![x1.into(), y1.into(), x2.into(), y2.into()]);
    let rect = Rectangle::try_from(&arr).unwrap();
    (rect.left_x, rect.lower_y, rect.right_x, rect.upper_y)
}

#[test]
fn rectangle_to_skia() {
    let rect = Rectangle::from_xywh(98.0, 519.0, 423.0, -399.0);
    let skia_rect: tiny_skia::Rect = rect.into();
    assert_eq!(
        skia_rect,
        tiny_skia::Rect::from_ltrb(98.0, 519.0 - 399.0, 98.0 + 423.0, 519.0).unwrap()
    );
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
        let mut dict = Dictionary::new();
        dict.insert(
            "Type".into(),
            (if kids.is_empty() { "/Page" } else { "/Pages" }).into(),
        );
        dict.insert(
            "MediaBox".into(),
            vec![0.0.into(), 0.0.into(), 0.0.into(), 0.0.into()].into(),
        );
        dict.insert(
            "Kids".into(),
            Object::Array(kids.into_iter().map(Object::new_ref).collect::<Array>()),
        );
        resolver.setup_object(id, Object::Dictionary(dict));
    }

    let pages = Page::parse(
        resolver
            .resolve_pdf_object(NonZeroU32::new(root_id).unwrap())
            .unwrap(),
    );
    pages.unwrap().into_iter().map(|p| p.id()).collect()
}
