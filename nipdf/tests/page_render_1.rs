//! Test page render result using `insta` to ensure that the rendering result is not changed.
//! This file checks file pdfReferenceUpdated.pdf
use anyhow::Result as AnyResult;
use hex::ToHex;
use insta::assert_ron_snapshot;
use md5::{Digest, Md5};
use nipdf::file::{File, RenderOptionBuilder};
use nipdf_test_macro::pdf_file_test_cases;
use reqwest::blocking::get as download;
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
/// touch a flag file at `$CARGO_TARGET_TMPDIR/(md5(f)).ok` if succeed.
/// If the file exist, skips the test.
///
/// If f ends with ".link", file content is a http url, download
/// that file to `$flag_file.pdf`, skip the download if `$flag_file.pdf` exists.
#[pdf_file_test_cases]
fn render(f: &str) -> AnyResult<()> {
    let hash_file: String = Md5::digest(f.as_bytes()).as_slice().encode_hex();
    let mut hash_file = Path::join(Path::new(env!["CARGO_TARGET_TMPDIR"]), hash_file);
    hash_file.set_extension("ok");
    let hash_file = hash_file;
    if hash_file.exists() {
        return Ok(());
    }

    let mut file_path = f;
    let mut pdf_file: PathBuf;
    if f.ends_with(".link") {
        pdf_file = hash_file.clone();
        pdf_file.set_extension("pdf");
        file_path = pdf_file.to_str().unwrap();
        if !pdf_file.exists() {
            let url = std::fs::read_to_string(f)?;
            let url = url.trim();
            let mut err = Ok(0u64);
            for url in url.split("http").into_iter().filter(|f| !f.is_empty()) {
                let url = format!("http{}", url);
                err = download(url).and_then(|mut resp| {
                    let mut f = std::fs::File::create(&pdf_file).unwrap();
                    resp.copy_to(&mut f)
                });
                if err.is_ok() {
                    break;
                }
            }
            if err.is_err() {
                return Err(err.unwrap_err().into());
            }
        }
    }

    let buf = std::fs::read(file_path).unwrap();
    let pdf = File::parse(buf, "", "").unwrap_or_else(|_| panic!("failed to parse {f:?}"));
    let resolver = pdf.resolver().unwrap();
    let catalog = pdf.catalog(&resolver)?;
    for page in catalog.pages()? {
        let option = RenderOptionBuilder::new().zoom(0.75);
        page.render(option)?;
    }
    std::fs::write(&hash_file, "")?;

    Ok(())
}
