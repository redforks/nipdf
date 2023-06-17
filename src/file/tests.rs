use crate::object::Object;
use crate::parser::{parse_frame_set, parse_header};
use std::path::PathBuf;
use std::str::from_utf8;
use test_case::test_case;

use super::*;

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

#[test]
fn xref_table_resolve_object_buf() {
    let buf = b"1234567890";
    let mut id_offset = IDOffsetMap::with_hasher(BuildNoHashHasher::default());
    id_offset.insert(1, 5);
    id_offset.insert(2, 3);
    let xref_table = XRefTable::new(buf, id_offset);

    assert_eq!(xref_table.resolve_object_buf(1), Some(&b"67890"[..]));
    assert_eq!(xref_table.resolve_object_buf(2), Some(&b"4567890"[..]));
    assert_eq!(xref_table.resolve_object_buf(3), None);
}

#[test]
fn object_resolver() {
    let buf = b"   5 null 2 0 R";
    let mut id_offset = IDOffsetMap::with_hasher(BuildNoHashHasher::default());
    id_offset.insert(1, 5);
    id_offset.insert(2, 3);
    id_offset.insert(3, 10);
    let xref_table = XRefTable::new(buf, id_offset);
    let mut resolver = ObjectResolver::new(xref_table);

    assert_eq!(resolver.resolve(1), Some(&Object::Null));
    assert_eq!(resolver.resolve(2), Some(&Object::Integer(5)));
    assert_eq!(resolver.resolve(3), Some(&Object::Integer(5)));
}

#[test]
fn parse_file() {
    let mut p = PathBuf::from(file!());
    assert_eq!(
        p.pop().then(|| p.pop().then(|| p.pop())).flatten(),
        Some(true)
    );
    p.push("sample_files");
    p.push("normal");
    p.push("SamplePdf1_12mb_6pages.pdf");
    let buf = std::fs::read(p).unwrap();
    let (_, head_ver) = parse_header(&buf[..]).unwrap();
    let (_, frame_set) = parse_frame_set(&buf[..]).unwrap();
    let xref = XRefTable::from_frame_set(&buf[..], &frame_set);
    let mut object_resolver = ObjectResolver::new(xref);
    let f = File::parse(head_ver.to_owned(), &frame_set, &mut object_resolver).unwrap();
    assert_eq!("1.5", f.version());
}
