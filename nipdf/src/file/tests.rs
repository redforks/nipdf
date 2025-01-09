use super::*;
use crate::object::{Object, SchemaDict};
use prescript::{AnyWhatever, sname};
use snafu::report;
use std::path::PathBuf;

#[test]
fn xref_table_resolve_object_buf() {
    let buf = b"1234567890";
    let mut id_offset = IDOffsetMap::default();
    id_offset.insert(1.into(), ObjectPos::Offset(5));
    id_offset.insert(2.into(), ObjectPos::Offset(3));
    let xref_table = XRefTable::new(id_offset);

    assert_eq!(
        xref_table.resolve_object_buf(buf, 1, None).unwrap(),
        Either::Left(&b"67890"[..])
    );
    assert_eq!(
        xref_table.resolve_object_buf(buf, 2, None).unwrap(),
        Either::Left(&b"4567890"[..])
    );
    assert!(matches!(
        xref_table.resolve_object_buf(buf, 3, None),
        Err(ObjectValueError::ObjectIDNotFound { id }) if id == 3.into()
    ));
}

#[report]
#[test]
fn object_resolver() -> Result<(), ObjectValueError> {
    let buf = b"   2 0 obj 5 endobj 1 0 obj null endobj 3 0 obj 2 0 R endobj";
    let mut id_offset = IDOffsetMap::default();
    id_offset.insert(1.into(), ObjectPos::Offset(20));
    id_offset.insert(2.into(), ObjectPos::Offset(3));
    id_offset.insert(3.into(), ObjectPos::Offset(40));
    let xref_table = XRefTable::new(id_offset);
    let resolver = ObjectResolver::new(buf, &xref_table, None);

    std::assert_eq!(
        resolver
            .resolve(1)
            .whatever_context::<_, ObjectValueError>("1")?,
        &Object::Null
    );
    std::assert_eq!(resolver.resolve(2)?, &Object::Integer(5));
    std::assert_eq!(resolver.resolve(1)?, &Object::Null);
    Ok(())
}

#[test]
fn object_resolver_resolve_container_value() {
    let dict = b"<</a 1>>";
    let dict = parser::dict::<_, ContextError>.parse(&dict[..]).unwrap();
    let xref = XRefTable::empty();
    let resolver = ObjectResolver::empty(&xref);

    assert_eq!(
        resolver
            .do_resolve_container_value(&dict, &sname("a"))
            .unwrap(),
        (None, &Object::Integer(1))
    );
    assert!(matches!(
        resolver.resolve_container_value(&dict, &sname("b")),
        Err(ObjectValueError::DictKeyNotFound)
    ));
}

#[pdf_object(())]
trait FooDictTrait {}

#[test]
fn resolve_container_one_or_more_pdf_object() -> Result<()> {
    // field not exist
    let buf = br#"1 0 obj
<<>>
endobj
"#;
    let xref = XRefTable::from_buf(buf)?;
    let resolver = ObjectResolver::new(buf, &xref, None);
    let d = resolver.resolve(1).unwrap().as_dict().unwrap();
    let d = SchemaDict::new(d, &resolver, ()).unwrap();
    assert!(
        d.resolve_one_or_more_pdf_object::<FooDict<'_, '_>>(&sname("foo"))
            .unwrap()
            .is_empty()
    );

    // field is dictionary
    let buf = br#"1 0 obj
<</foo 2 0 R>>
endobj
2 0 obj<<>>endobj
"#;
    let xref = XRefTable::from_buf(buf)?;
    let resolver = ObjectResolver::new(buf, &xref, None);
    let d = resolver.resolve(1).unwrap().as_dict().unwrap();
    let d = SchemaDict::new(d, &resolver, ()).unwrap();
    let list = d
        .resolve_one_or_more_pdf_object::<FooDict<'_, '_>>(&sname("foo"))
        .unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(Some(2.into()), list[0].id());

    // field is array
    let buf = br#"1 0 obj
<</foo [<<>> 3 0 R]>>
endobj
3 0 obj<<>>endobj
"#;
    let xref = XRefTable::from_buf(buf)?;
    let resolver = ObjectResolver::new(buf, &xref, None);
    let d = resolver.resolve(1).unwrap().as_dict().unwrap();
    let d = SchemaDict::new(d, &resolver, ()).unwrap();
    let list = d
        .resolve_one_or_more_pdf_object::<FooDict<'_, '_>>(&sname("foo"))
        .unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(None, list[0].id());
    assert_eq!(Some(3.into()), list[1].id());

    Ok(())
}

#[report]
#[test]
fn resolve_one_or_more_pdf_object() -> Result<(), AnyWhatever> {
    // object is dictionary
    let buf = b"1 0 obj <</foo 2 0 R>> endobj";
    let xref = XRefTable::from_buf(buf)?;
    let resolver = ObjectResolver::new(buf, &xref, None);
    let id = Object::new_ref(1);
    let list = resolver
        .resolve_one_or_more_pdf_object::<FooDict<'_, '_>>(&id)
        .unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(Some(1.into()), list[0].id());

    // object is stream
    let buf = br#"1 0 obj <</foo 2 0 R/Length 3>> stream
123
endstream
endobj"#;
    let xref = XRefTable::from_buf(buf)?;
    let resolver = ObjectResolver::new(buf, &xref, None);
    let id = Object::new_ref(1);
    let list = resolver
        .resolve_one_or_more_pdf_object::<FooDict<'_, '_>>(&id)
        .whatever_context("resolve FooDict")?;
    assert_eq!(list.len(), 1);
    assert_eq!(Some(1.into()), list[0].id());

    // object is array
    let buf = br#"1 0 obj [2 0 R<<>>] endobj
    2 0 obj<<>>endobj"#;
    let xref = XRefTable::from_buf(buf)?;
    let resolver = ObjectResolver::new(buf, &xref, None);
    let id = Object::new_ref(1);
    let list = resolver
        .resolve_one_or_more_pdf_object::<FooDict<'_, '_>>(&id)
        .whatever_context("Resolve FooDict")?;
    assert_eq!(list.len(), 2);
    assert_eq!(Some(2.into()), list[0].id());
    assert_eq!(None, list[1].id());

    Ok(())
}

#[report]
#[test]
fn parse_file() -> Result<()> {
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
    let buf = std::fs::read(p).whatever_context("read file")?;
    let f = File::parse(buf, "").whatever_context("parse pdf file")?;
    let resolver = f.resolver()?;
    assert_eq!(
        Some("1.5".to_owned()),
        f.version(&resolver).whatever_context("version")?
    );

    Ok(())
}
