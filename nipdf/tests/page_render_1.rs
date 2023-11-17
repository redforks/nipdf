//! Test page render result using `insta` to ensure that the rendering result is not changed.
//! This file checks file pdfReferenceUpdated.pdf
use anyhow::Result as AnyResult;
use hex::ToHex;
use insta::assert_ron_snapshot;
use md5::{Digest, Md5};
use nipdf::file::{File, RenderOptionBuilder};
use nipdf_test_macro::file_render_test;
use std::{
    num::NonZeroU32,
    path::{Path, PathBuf},
};
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

/// Read pdf file and render each page, to save test time,
/// touch a file at `$CARGO_TARGET_TMPDIR/(md5(f))` if succeed.
/// If the file exist, skips the test
#[pdf_file_test_cases]
fn render(f: &str) -> AnyResult<()> {
    let hash_file: String = Md5::digest(f.as_bytes()).as_slice().encode_hex();
    let hash_file = Path::join(Path::new(env!["CARGO_TARGET_TMPDIR"]), hash_file);
    if hash_file.exists() {
        return Ok(());
    }

    let path = PathBuf::from(f);
    let buf = std::fs::read(&path).unwrap();
    let f = File::parse(buf, "", "").unwrap_or_else(|_| panic!("failed to parse {path:?}"));
    let resolver = f.resolver().unwrap();
    let catalog = f.catalog(&resolver)?;
    for page in catalog.pages()? {
        let option = RenderOptionBuilder::new().zoom(0.75);
        page.render(option)?;
    }
    Ok(std::fs::write(&hash_file, "")?)
}
