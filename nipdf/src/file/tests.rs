use super::*;
use crate::{
    object::{Object, SchemaDict},
    parser::parse_dict,
};
use prescript::sname;
use std::path::PathBuf;

#[test]
fn xref_table_resolve_object_buf() {
    let buf = b"1234567890";
    let mut id_offset = IDOffsetMap::default();
    id_offset.insert(1.into(), ObjectPos::Offset(5));
    id_offset.insert(2.into(), ObjectPos::Offset(3));
    let xref_table = XRefTable::new(id_offset);

    assert_eq!(
        xref_table.resolve_object_buf(buf, 1, None),
        Some(Either::Left(&b"67890"[..]))
    );
    assert_eq!(
        xref_table.resolve_object_buf(buf, 2, None),
        Some(Either::Left(&b"4567890"[..]))
    );
    assert_eq!(xref_table.resolve_object_buf(buf, 3, None), None);
}

#[test]
fn object_resolver() {
    let buf = b"   2 0 obj 5 endobj 1 0 obj null endobj 3 0 obj 2 0 R endobj";
    let mut id_offset = IDOffsetMap::default();
    id_offset.insert(1.into(), ObjectPos::Offset(20));
    id_offset.insert(2.into(), ObjectPos::Offset(3));
    id_offset.insert(3.into(), ObjectPos::Offset(40));
    let xref_table = XRefTable::new(id_offset);
    let resolver = ObjectResolver::new(buf, &xref_table, None);

    assert_eq!(resolver.resolve(1), Ok(&Object::Null));
    assert_eq!(resolver.resolve(2), Ok(&Object::Integer(5)));
    assert_eq!(resolver.resolve(1), Ok(&Object::Null));
}

#[test]
fn object_resolver_resolve_container_value() {
    let dict = b"<</a 1>>";
    let (_, dict) = parse_dict(dict).unwrap();
    let xref = XRefTable::empty();
    let resolver = ObjectResolver::empty(&xref);

    assert_eq!(
        resolver
            .do_resolve_container_value(&dict, &sname("a"))
            .unwrap(),
        (None, &Object::Integer(1))
    );
    assert_eq!(
        Err(ObjectValueError::DictKeyNotFound),
        resolver.resolve_container_value(&dict, &sname("b"))
    );
}

#[pdf_object(())]
trait FooDictTrait {}

#[test]
fn resolve_container_one_or_more_pdf_object() -> AnyResult<()> {
    // field not exist
    let buf = br#"1 0 obj
<<>>
endobj
"#;
    let xref = XRefTable::from_buf(buf);
    let resolver = ObjectResolver::new(buf, &xref, None);
    let d = resolver.resolve(1)?.as_dict()?;
    let d = SchemaDict::new(d, &resolver, ())?;
    assert!(
        d.resolve_one_or_more_pdf_object::<FooDict>(&sname("foo"))?
            .is_empty()
    );

    // field is dictionary
    let buf = br#"1 0 obj
<</foo 2 0 R>>
endobj
2 0 obj<<>>endobj
"#;
    let xref = XRefTable::from_buf(buf);
    let resolver = ObjectResolver::new(buf, &xref, None);
    let d = resolver.resolve(1)?.as_dict()?;
    let d = SchemaDict::new(d, &resolver, ())?;
    let list = d.resolve_one_or_more_pdf_object::<FooDict>(&sname("foo"))?;
    assert_eq!(list.len(), 1);
    assert_eq!(Some(2.into()), list[0].id());

    // field is array
    let buf = br#"1 0 obj
<</foo [<<>> 3 0 R]>>
endobj
3 0 obj<<>>endobj
"#;
    let xref = XRefTable::from_buf(buf);
    let resolver = ObjectResolver::new(buf, &xref, None);
    let d = resolver.resolve(1)?.as_dict()?;
    let d = SchemaDict::new(d, &resolver, ())?;
    let list = d.resolve_one_or_more_pdf_object::<FooDict>(&sname("foo"))?;
    assert_eq!(list.len(), 2);
    assert_eq!(None, list[0].id());
    assert_eq!(Some(3.into()), list[1].id());

    Ok(())
}

#[test]
fn resolve_one_or_more_pdf_object() {
    // object is dictionary
    let buf = b"1 0 obj <</foo 2 0 R>> endobj";
    let xref = XRefTable::from_buf(buf);
    let resolver = ObjectResolver::new(buf, &xref, None);
    let id = Object::new_ref(1);
    let list = resolver
        .resolve_one_or_more_pdf_object::<FooDict>(&id)
        .unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(Some(1.into()), list[0].id());

    // object is stream
    let buf = br#"1 0 obj <</foo 2 0 R/Length 3>> stream
123
endstream
endobj"#;
    let xref = XRefTable::from_buf(buf);
    let resolver = ObjectResolver::new(buf, &xref, None);
    let id = Object::new_ref(1);
    let list = resolver
        .resolve_one_or_more_pdf_object::<FooDict>(&id)
        .unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(Some(1.into()), list[0].id());

    // object is array
    let buf = br#"1 0 obj [2 0 R<<>>] endobj
2 0 obj<<>>endobj"#;
    let xref = XRefTable::from_buf(buf);
    let resolver = ObjectResolver::new(buf, &xref, None);
    let id = Object::new_ref(1);
    let list = resolver
        .resolve_one_or_more_pdf_object::<FooDict>(&id)
        .unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(Some(2.into()), list[0].id());
    assert_eq!(None, list[1].id());
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
    let f = File::parse(buf, "", "").unwrap();
    let resolver = f.resolver().unwrap();
    assert_eq!(Some("1.5".to_owned()), f.version(&resolver).unwrap());
}
