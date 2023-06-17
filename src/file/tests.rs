use crate::object::Object;
use std::{collections::BTreeMap, str::from_utf8};
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
