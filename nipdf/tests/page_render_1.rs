//! Test page render result using `insta` to ensure that the rendering result is not changed.
//! This file checks file pdfReferenceUpdated.pdf
use anyhow::Result as AnyResult;
use insta::assert_ron_snapshot;
use md5::{Digest, Md5};
use nipdf::file::File;
use nipdf_test_macro::file_render_test;
use std::num::NonZeroU32;
use test_case::test_case;

/// Decode pdf embed image and return the result as Vec<u8>.
/// The image is specified by ref id.
fn decode_image(id: u32) -> AnyResult<String> {
    let path = "sample_files/bizarre/pdfReferenceUpdated.pdf";
    let buf = std::fs::read(path)?;
    let f = File::parse(buf, "", "").unwrap_or_else(|_| panic!("failed to parse {path:?}"));
    let resolver = f.resolver()?;
    let obj = resolver.resolve(NonZeroU32::new(id).unwrap())?;
    let image = obj.as_stream()?.decode_image(&resolver, None)?;
    let hash = Md5::digest(image.into_bytes());
    Ok(hex::encode(hash))
}

#[test]
fn image_separation_color_space() {
    // image dict has ColorSpace entry, Separation with alternate color space DeviceCMYK,
    // test if the image pixels colors transformed correctly
    // image 1297 used in page 488(from zero), page resource image name: Im3
    assert_ron_snapshot!(&decode_image(1297).unwrap());
}

#[file_render_test]
fn render(f: &str) {
    dbg!(f);
}
