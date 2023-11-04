use crate::object::{Object, SchemaDict};

use std::path::PathBuf;

use super::*;

#[test]
fn xref_table_resolve_object_buf() {
    let buf = b"1234567890";
    let mut id_offset = IDOffsetMap::default();
    id_offset.insert(1, ObjectPos::Offset(5));
    id_offset.insert(2, ObjectPos::Offset(3));
    let xref_table = XRefTable::new(id_offset);

    assert_eq!(
        xref_table.resolve_object_buf(buf, to_non_zero_u32(1)),
        Some(Either::Left(&b"67890"[..]))
    );
    assert_eq!(
        xref_table.resolve_object_buf(buf, to_non_zero_u32(2)),
        Some(Either::Left(&b"4567890"[..]))
    );
    assert_eq!(xref_table.resolve_object_buf(buf, to_non_zero_u32(3)), None);
}

fn to_non_zero_u32(v: u32) -> NonZeroU32 {
    NonZeroU32::new(v).unwrap()
}

#[test]
fn object_resolver() {
    let buf = b"   2 0 obj 5 endobj 1 0 obj null endobj 3 0 obj 2 0 R endobj";
    let mut id_offset = IDOffsetMap::default();
    id_offset.insert(1, ObjectPos::Offset(20));
    id_offset.insert(2, ObjectPos::Offset(3));
    id_offset.insert(3, ObjectPos::Offset(40));
    let xref_table = XRefTable::new(id_offset);
    let resolver = ObjectResolver::new(buf, &xref_table);

    assert_eq!(resolver.resolve(to_non_zero_u32(1)), Ok(&Object::Null));
    assert_eq!(
        resolver.resolve(to_non_zero_u32(2)),
        Ok(&Object::Integer(5))
    );
    assert_eq!(
        resolver.resolve(to_non_zero_u32(3)),
        Ok(&Object::Integer(5))
    );
    assert_eq!(resolver.resolve(to_non_zero_u32(1)), Ok(&Object::Null));
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
    let resolver = ObjectResolver::new(buf, &xref);
    let d = resolver
        .resolve(NonZeroU32::new(1_u32).unwrap())?
        .as_dict()?;
    let d = SchemaDict::new(d, &resolver, ())?;
    assert!(d
        .resolve_container_one_or_more_pdf_object::<FooDict>("foo")?
        .is_empty());

    // field is dictionary
    let buf = br#"1 0 obj
<</foo 2 0 R>>
endobj
2 0 obj<<>>endobj
"#;
    let xref = XRefTable::from_buf(buf);
    let resolver = ObjectResolver::new(buf, &xref);
    let d = resolver
        .resolve(NonZeroU32::new(1_u32).unwrap())?
        .as_dict()?;
    let d = SchemaDict::new(d, &resolver, ())?;
    let list = d.resolve_container_one_or_more_pdf_object::<FooDict>("foo")?;
    assert_eq!(list.len(), 1);
    assert_eq!(Some(NonZeroU32::new(2).unwrap()), list[0].id());

    // field is array
    let buf = br#"1 0 obj
<</foo [<<>> 3 0 R]>>
endobj
3 0 obj<<>>endobj
"#;
    let xref = XRefTable::from_buf(buf);
    let resolver = ObjectResolver::new(buf, &xref);
    let d = resolver
        .resolve(NonZeroU32::new(1_u32).unwrap())?
        .as_dict()?;
    let d = SchemaDict::new(d, &resolver, ())?;
    let list = d.resolve_container_one_or_more_pdf_object::<FooDict>("foo")?;
    assert_eq!(list.len(), 2);
    assert_eq!(None, list[0].id());
    assert_eq!(Some(NonZeroU32::new(3).unwrap()), list[1].id());

    Ok(())
}

#[test]
fn resolve_one_or_more_pdf_object() {
    // object is dictionary
    let buf = b"1 0 obj <</foo 2 0 R>> endobj";
    let xref = XRefTable::from_buf(buf);
    let resolver = ObjectResolver::new(buf, &xref);
    let list = resolver
        .resolve_one_or_more_pdf_object::<FooDict>(NonZeroU32::new(1).unwrap())
        .unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(Some(NonZeroU32::new(1).unwrap()), list[0].id());

    // object is stream
    let buf = br#"1 0 obj <</foo 2 0 R/Length 3>> stream
123
endstream
endobj"#;
    let xref = XRefTable::from_buf(buf);
    let resolver = ObjectResolver::new(buf, &xref);
    let list = resolver
        .resolve_one_or_more_pdf_object::<FooDict>(NonZeroU32::new(1).unwrap())
        .unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(Some(NonZeroU32::new(1).unwrap()), list[0].id());

    // object is array
    let buf = br#"1 0 obj [2 0 R<<>>] endobj
2 0 obj<<>>endobj"#;
    let xref = XRefTable::from_buf(buf);
    let resolver = ObjectResolver::new(buf, &xref);
    let list = resolver
        .resolve_one_or_more_pdf_object::<FooDict>(NonZeroU32::new(1).unwrap())
        .unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(Some(NonZeroU32::new(2).unwrap()), list[0].id());
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
    let f = File::parse(buf).unwrap();
    let resolver = f.resolver().unwrap();
    assert_eq!("1.5", f.version(&resolver).unwrap());
}
