//! Test page render result using `insta` to ensure that the rendering result is not changed.
//! This file checks file pdfReferenceUpdated.pdf
use anyhow::Result as AnyResult;
use hex::ToHex;
use insta::assert_ron_snapshot;
use log::info;
use maplit::hashmap;
use md5::{Digest, Md5};
use nipdf::file::File;
use nipdf_render::{render_page, RenderOptionBuilder};
use nipdf_test_macro::pdf_file_test_cases;
use std::{
    collections::hash_map::HashMap,
    io::BufWriter,
    path::{Path, PathBuf},
};
use test_case::test_case;
use ureq::get as download;

/// Decode pdf embed image and return the result as Vec<u8>.
/// The image is specified by ref id.
fn decode_image(id: u32) -> AnyResult<String> {
    let path = "../nipdf/sample_files/bizarre/pdfReferenceUpdated.pdf";
    let buf = std::fs::read(path)?;
    let f = File::parse(buf, "", "").unwrap_or_else(|_| panic!("failed to parse {path:?}"));
    let resolver = f.resolver()?;
    let obj = resolver.resolve(id)?;
    let image = obj.stream()?.decode_image(&resolver, None)?;
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

/// Some link files point to dead link, replace with alternative download url
fn replace_dead_link(f: &str) -> Option<&'_ str> {
    let dead_links: HashMap<&str, &str> = hashmap! {
        "bpl13210.pdf.link" => "https://raw.githubusercontent.com/Hehouhua/papers_read/master/bpl13210.pdf",
        "artofwar.pdf.link" => "http://www.thegoyslife.com/Documents/Books/ArtofWarbySunTzu.pdf",
    };
    let p = Path::new(f);
    dead_links.get(p.file_name()?.to_str()?).copied()
}

fn download_file(url: &str, p: impl AsRef<Path>) -> AnyResult<()> {
    let resp = download(url).call()?;
    let f = std::fs::File::create(p.as_ref())?;
    let mut f = BufWriter::new(f);
    let mut resp = resp.into_reader();
    if let Err(err) = std::io::copy(&mut resp, &mut f) {
        // delete the file if download failed
        std::fs::remove_file(p.as_ref())?;
        Err(err)?
    } else {
        Ok(())
    }
}

/// These files are very rare and odd, not to be tested
const IGNORED: [&str; 9] = [
    // xpdf, mupdf, are all failed to open
    "bug1020226.pdf",
    // odd FlateDecode stream, xpdf failed to decode, mupdf no problem
    "bug1050040.pdf",
    // the link is dead
    "bug1037816.pdf.link",
    // corrupt content at the end of file, xpdf, mupdf okay, qpdf failed(although it tried to
    // recover) see: xpdf/PDFDoc.cc PDFDoc::setup2()
    "bug1250079.pdf",
    // incorrect startxref pos, and incorrect page content operations
    "bug1130815.pdf.link",
    // Contains JBIG2Decode stream, an image format likes CCITT, only one crate can decode JBig2:
    // `jbig2dec`, ffi to `jbig2dec` written in C, which is AGPL license.
    // `xpdf` and `pdf.js` implement its own decoder. AGPL can link with Apache libs
    // I'll dive into this after other features implemented. Crate `jbig2dec` need forked to
    // support JBIG2 Global stream
    "bug1064894.pdf.link",
    // damaged file, xpdf/mupdf/qpdf all failed to open,
    // maybe can be read using Linearized PDF (Annex F)
    "bpl13210.pdf.link",
    // incorrect startxref pos, need to fix xref.
    // xpdf says: attempting to reconstruct xref table...
    "bug1606566.pdf",
    // todo: render Type 4 Shadings
    "bug1260585.pdf.link",
];

/// Read pdf file and render each page, to save test time,
/// touch a flag file at `$CARGO_TARGET_TMPDIR/(md5(f)).ok` if succeed.
/// If the file exist, skips the test.
///
/// If f ends with ".link", file content is a http url, download
/// that file to `$flag_file.pdf`, skip the download if `$flag_file.pdf` exists.
#[pdf_file_test_cases]
fn render(f: &str) -> AnyResult<()> {
    // return if f ends with one of IGNORED
    if IGNORED.iter().any(|s| f.ends_with(s)) {
        return Ok(());
    }

    let hash_file: String = Md5::digest(f.as_bytes()).as_slice().encode_hex();
    let mut hash_file = Path::join(Path::new(env!["CARGO_TARGET_TMPDIR"]), hash_file);
    eprintln!("{}.pdf", hash_file.to_str().unwrap());
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
            if let Some(link) = replace_dead_link(f) {
                download_file(link, &pdf_file)?;
            } else {
                let url = std::fs::read_to_string(f)?;
                let url = url.trim();
                let mut err = Ok(());
                for url in url.split("http").filter(|f| !f.is_empty()) {
                    let url = format!("http{}", url);
                    err = download_file(&url, &pdf_file);
                    if err.is_ok() {
                        break;
                    }
                }
                err?
            }
        }
    }

    let buf = std::fs::read(file_path).unwrap();
    let pdf = File::parse(buf, "", "").unwrap_or_else(|_| panic!("failed to parse {f:?}"));
    let resolver = pdf.resolver().unwrap();
    let catalog = pdf.catalog(&resolver)?;
    for (idx, page) in catalog.pages()?.into_iter().enumerate() {
        info!("Page: {}", idx);
        let option = RenderOptionBuilder::new().zoom(0.75);
        render_page(&page, option)?;
    }
    std::fs::write(&hash_file, "")?;

    Ok(())
}
