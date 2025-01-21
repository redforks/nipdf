use super::*;
use crate::object::Object;
use snafu::report;
use std::path::PathBuf;

#[test]
fn xref_table_resolve_object_buf() {
    let buf = b"1234567890";
    let mut id_offset = HashMap::default();
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
    let mut id_offset = HashMap::default();
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

#[pdf_object(())]
#[root_pdf_object]
trait RootFooDictTrait {}

#[pdf_object(())]
trait FooDictTrait {}

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
    let buf = std::fs::read(p).whatever_context::<_, ObjectValueError>("read file")?;
    let f = File::parse(buf, "").whatever_context::<_, ObjectValueError>("parse pdf file")?;
    let resolver = f.resolver()?;
    assert_eq!(
        Some("1.5".to_owned()),
        f.version(&resolver)
            .whatever_context::<_, ObjectValueError>("version")?
    );

    Ok(())
}

#[report]
#[test]
fn build_xref() -> Result<()> {
    let f = test_file("pdf.js/test/pdfs/helloworld-bad.pdf");
    let buf = std::fs::read(&f).whatever_context::<_, ObjectValueError>("read file")?;
    let f = File::parse(buf, "").whatever_context::<_, ObjectValueError>("build xref")?;
    let resolver = f.resolver()?;
    // now it is ok.
    for i in 1..resolver.n() {
        resolver
            .resolve(i as u32)
            .whatever_context::<_, ObjectValueError>("resolve object")?;
    }

    Ok(())
}
