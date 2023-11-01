//! Test page render result using `insta` to ensure that the rendering result is not changed.
//! This file checks file pdfreference1.0.pdf
use std::path::Path;

use anyhow::Result as AnyResult;
use insta::assert_ron_snapshot;
use md5::{Digest, Md5};
use nipdf::file::{File, RenderOptionBuilder};

fn decode_file_page(path: &str, page_no: usize) -> AnyResult<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    let buf = std::fs::read(&path)?;
    let f = File::parse(buf).unwrap_or_else(|_| panic!("failed to parse {path:?}"));
    let resolver = f.resolver()?;
    let catalog = f.catalog(&resolver)?;
    let pages = catalog.pages()?;
    let page = &pages[page_no];
    let option = RenderOptionBuilder::new().zoom(1.5);
    let bytes = page.render(option)?.take();
    let hash = Md5::digest(&bytes[..]);
    Ok(hex::encode(hash))
}

/// Render page to image, and returns its md5 hash converted to hex
fn decode_page(page_no: usize) -> AnyResult<String> {
    decode_file_page("sample_files/normal/pdfreference1.0.pdf", page_no)
}

#[test]
fn clip_mask() {
    assert_ron_snapshot!(&decode_page(141).unwrap());
}

#[test]
fn mask_image() {
    assert_ron_snapshot!(&decode_page(163).unwrap());
}

#[test]
fn pattern_color() {
    assert_ron_snapshot!(
        &decode_file_page("sample_files/normal/SamplePdf1_12mb_6pages.pdf", 5).unwrap()
    );
}

#[test]
fn form() {
    assert_ron_snapshot!(&decode_file_page("sample_files/xobject/form.pdf", 0).unwrap());
}
