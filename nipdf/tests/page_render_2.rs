//! Test page render result using `insta` to ensure that the rendering result is not changed.
//! This file checks file pdfreference1.0.pdf
use anyhow::Result as AnyResult;
use insta::assert_ron_snapshot;
use md5::{Digest, Md5};
use nipdf::file::{File, ObjectResolver, RenderOptionBuilder};

/// Render page to image, and returns its md5 hash converted to hex
fn decode_page(page_no: usize) -> AnyResult<String> {
    let path = "sample_files/normal/pdfreference1.0.pdf";
    let buf = std::fs::read(path)?;
    let (f, xref) = File::parse(&buf[..]).unwrap_or_else(|_| panic!("failed to parse {path:?}"));
    let resolver = ObjectResolver::new(&buf[..], &xref);
    let catalog = f.catalog(&resolver)?;
    let pages = catalog.pages()?;
    let page = &pages[page_no];
    let option = RenderOptionBuilder::new().zoom(1.5);
    let bytes = page.render(option)?.take();
    let hash = Md5::digest(&bytes[..]);
    Ok(hex::encode(hash))
}

#[test]
fn clip_mask() {
    assert_ron_snapshot!(&decode_page(141).unwrap());
}
