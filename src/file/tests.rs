use std::{collections::BTreeMap, str::from_utf8};
use test_case::test_case;

use super::*;

#[test]
fn resolve_xref_table() {
    let mut section1 = BTreeMap::new();
    let entry1_0 = Entry::new(0, 65535, false);
    let entry1_1 = Entry::new(1, 0, true);
    let entry1_2 = Entry::new(2, 0, true);
    let entry1_3 = Entry::new(3, 0, true);
    section1.insert(0, entry1_0);
    section1.insert(1, entry1_1);
    section1.insert(2, entry1_2);
    section1.insert(3, entry1_3);
    let section1 = Section::new(section1);
    let frame1 = Frame::new(100, Trailer::new(Dictionary::new()), section1);

    let mut section2 = BTreeMap::new();
    let entry2_2 = Entry::new(100, 1, true);
    let entry2_3 = Entry::new(200, 1, false);
    section2.insert(2, entry2_2);
    section2.insert(3, entry2_3);
    let section2 = Section::new(section2);
    let frame2 = Frame::new(101, Trailer::new(Dictionary::new()), section2);

    let mut section3 = BTreeMap::new();
    let entry3_1 = Entry::new(102, 1, false);
    let entry3_3 = Entry::new(300, 2, true);
    section3.insert(1, entry3_1);
    section3.insert(3, entry3_3);
    let section3 = Section::new(section3);
    let frame3 = Frame::new(102, Trailer::new(Dictionary::new()), section3);

    let frame_set = FrameSet::new(vec![frame3, frame2, frame1]);

    // resolve not exist
    assert_eq!(frame_set.resolve_object(10), None);
    assert_eq!(frame_set.resolve_object(0), Some(entry1_0));
    assert_eq!(frame_set.resolve_object(1), Some(entry3_1));
    assert_eq!(frame_set.resolve_object(2), Some(entry2_2));
    assert_eq!(frame_set.resolve_object(3), Some(entry3_3));

    assert_eq!(
        frame_set.iter_entry_by_id(0).collect::<Vec<_>>(),
        vec![entry1_0]
    );
    assert_eq!(
        frame_set.iter_entry_by_id(1).collect::<Vec<_>>(),
        vec![entry3_1, entry1_1]
    );
}

#[test_case(None, None; "no prev")]
#[test_case(100, Object::Integer(100); "has prev")]
fn prev_frame(exp: impl Into<Option<u32>>, prev_value: impl Into<Option<Object<'static>>>) {
    let mut dict = Dictionary::new();
    if let Some(prev) = prev_value.into() {
        dict.insert(Name::new(b"/Prev"), prev);
    }
    let trailer = Trailer::new(dict);
    let frame = Frame::new(0, trailer, Section::new(BTreeMap::new()));
    assert_eq!(frame.prev(), exp.into());
}

#[test_case(None => Ok(None))]
#[test_case(Object::Integer(100) => Err(ObjectValueError::UnexpectedType))]
#[test_case(Object::Name(Name::new(b"/abc")) => Ok(Some("abc".into())))]
fn catalog_ver(
    ver: impl Into<Option<Object<'static>>>,
) -> Result<Option<String>, ObjectValueError> {
    let ver = ver.into();
    let mut dict = Dictionary::new();
    if let Some(ver) = ver {
        dict.insert(Name::new(b"/Version"), ver);
    }
    let cat = Catalog::new(dict);
    cat.ver()
        .map(|v| v.map(|v| from_utf8(v.as_ref()).unwrap().into()))
}
