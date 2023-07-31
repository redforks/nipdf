use crate::object::Object;

use std::path::PathBuf;

use super::*;

#[test]
fn xref_table_resolve_object_buf() {
    let buf = b"1234567890";
    let mut id_offset = IDOffsetMap::default();
    id_offset.insert(1, 5);
    id_offset.insert(2, 3);
    let xref_table = XRefTable::new(buf, id_offset);

    assert_eq!(xref_table.resolve_object_buf(1), Some(&b"67890"[..]));
    assert_eq!(xref_table.resolve_object_buf(2), Some(&b"4567890"[..]));
    assert_eq!(xref_table.resolve_object_buf(3), None);
}

#[test]
fn object_resolver() {
    let buf = b"   2 0 obj 5 endobj 1 0 obj null endobj 3 0 obj 2 0 R endobj";
    let mut id_offset = IDOffsetMap::default();
    id_offset.insert(1, 20);
    id_offset.insert(2, 3);
    id_offset.insert(3, 40);
    let xref_table = XRefTable::new(buf, id_offset);
    let resolver = ObjectResolver::new(xref_table);

    assert_eq!(resolver.resolve(1), Ok(&Object::Null));
    assert_eq!(resolver.resolve(2), Ok(&Object::Integer(5)));
    assert_eq!(resolver.resolve(3), Ok(&Object::Integer(5)));
    assert_eq!(resolver.resolve(1), Ok(&Object::Null));
}

#[test]
fn parse_file() {
    let mut p = PathBuf::from(file!());
    assert_eq!(
        p.pop()
            .then(|| p.pop().then(|| p.pop().then(|| p.pop())))
            .flatten()
            .flatten(),
        Some(true)
    );
    p.push("sample_files");
    p.push("normal");
    p.push("SamplePdf1_12mb_6pages.pdf");
    let buf = std::fs::read(p).unwrap();
    let (f, resolver) = File::parse(&buf).unwrap();
    assert_eq!("1.5", f.version(&resolver).unwrap());
    assert_eq!(311, f.total_objects);
}
