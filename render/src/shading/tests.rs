use super::*;
use nipdf::{
    file::{ObjectResolver, XRefTable},
    object::Dictionary,
};
use snafu::Whatever;
use test_case::test_case;

#[test_case(b"1 0 obj<</ShadingType 3/ColorSpace/DeviceGray/Coords[1 1 0 1 1 0]>>endobj"; "radius both be zero")]
#[test_case(b"1 0 obj<</ShadingType 3/ColorSpace/DeviceGray/Coords[1 1 -1 1 1 1]>>endobj"; "negative start radius")]
#[test_case(b"1 0 obj<</ShadingType 3/ColorSpace/DeviceGray/Coords[1 1 1 1 1 -1]>>endobj"; "negative end radius")]
fn build_invalid_radial(buf: &[u8]) -> Result<(), Whatever> {
    let xref = XRefTable::from_buf(buf);
    let resolver = ObjectResolver::new(buf, &xref, None);
    let d: ShadingDict = resolver
        .resolve_pdf_object(1)
        .whatever_context("resolve pdf object")?;
    let empty_d = Dictionary::new();
    let resource = ResourceDict::new(None, &empty_d, &resolver)
        .whatever_context("create empty resource dict")?;
    assert_eq!(None, build_radial(&d, &resource)?);
    Ok(())
}
