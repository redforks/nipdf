#![allow(clippy::panic)]
#![allow(clippy::unwrap_used)]

//! Test page render result using `insta` to ensure that the rendering result is not changed.
//! This file checks file pdfReferenceUpdated.pdf
use hex::ToHex;
use insta::assert_ron_snapshot;
use log::info;
use maplit::hashmap;
use md5::{Digest, Md5};
use nipdf::file::File;
use nipdf_render::{RenderOptionBuilder, render_page};
use nipdf_test_macro::pdf_file_test_cases;
use prescript::Result;
use snafu::ResultExt as _;
use std::{
    collections::hash_map::HashMap,
    io::BufWriter,
    path::{Path, PathBuf},
};
use test_case::test_case;
use ureq::get as download;

/// Decode pdf embed image and return the result as Vec<u8>.
/// The image is specified by ref id.
fn decode_image(id: u32) -> Result<String> {
    let path = "../nipdf/sample_files/bizarre/pdfReferenceUpdated.pdf";
    let buf = std::fs::read(path).whatever_context("parse pdf file")?;
    let f = File::parse(buf, "").unwrap_or_else(|_| panic!("failed to parse {path:?}"));
    let resolver = f.resolver().whatever_context("get object resolver")?;
    let obj = resolver.resolve(id).whatever_context("resolve object")?;
    let image = obj
        .as_stream()
        .with_whatever_context(|_| format!("decode stream {id}"))?
        .decode_image(&resolver, None)
        .with_whatever_context(|_| format!("decode image {id}"))?;
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

fn download_file(url: &str, p: impl AsRef<Path>) -> Result<()> {
    info!("Download file: {}", url);
    let resp = download(url).call().whatever_context("download pdf file")?;
    let f = std::fs::File::create(p.as_ref()).whatever_context("create cache file")?;
    let mut f = BufWriter::new(f);
    let mut resp = resp.into_reader();
    std::io::copy(&mut resp, &mut f).whatever_context("save file")?;
    Ok(())
}

/// These files are very rare and odd, not to be tested
const IGNORED: [&str; 14] = [
    // odd FlateDecode stream, xpdf failed to decode, mupdf no problem
    "bug1050040.pdf",
    // invalid object format, mupdf failed to parse, mupdf says no page
    "bug1020226.pdf",
    // todo: render Type 4 Shadings
    "bug1260585.pdf.link",
    // contains jpeg2k image using cmyk color space,
    // `jpeg2k` crate failed handle it
    "bug1199237.pdf.link",
    // chrome failed to open this file. xpdf/mupdf works okay.
    // file end with document id that never complete.
    "bug1250079.pdf",
    // CMap stream /CM10 incorrect:
    //
    // ```
    // /CIDSystemInfo 3 dict dup begin
    // /Registry (Adobe) def
    // /Ordering (Identity) def
    // /Supplement 0 def
    //
    // % missing: "end def" here
    //
    // /CMapName /CM10 def
    // /CMapVersion 1.0 def
    // ```
    //
    // pdf.js/xpdf uses regex to extract information,
    // but I use full postscript VM,
    // I think the pdf file is create for this specific commit, and is incorrect,
    // it happened not trigger error in their code.
    // this kind of error should not exist in a normal pdf file that generate
    // by a pdf writer.
    // TODO: fix this pdf file after nipdf support write pdf file
    "bug920426.pdf",
    // Same as bug1260585.pdf.link
    "close-path-bug.pdf",
    // encrypted by pdf 2.0, Revision 6
    "empty_protected.pdf",
    // this file contains premature jpeg image(incomplete scan-line data),
    // jpeg-decoder failed to decode this file, I don't know how to use zune-jpeg
    // to decode this file with correct color.
    "issue11052.pdf.link",
    // the same as issue11052.pdf.link
    "issue1419.pdf.link",
    // the same as issue11052.pdf.link
    "issue1877.pdf.link",
    // this file contains invalid xref, after file scan trailer point to wrong catalog,
    // If resolve catalog dict by check all dict for `/Catalog` type, can find correct
    // catalog dict. `mupdf` also failed to parse this file, but others are okay.
    "issue12402.pdf.link",
    // xpdf, mupdf and chrome failed to parse this file, broken xref, failed get root entry after rebuild xref
    "issue15590.pdf",
    // ColorSpace CS0 is NULL, release version ignores failed operation, so release version is okay,
    "issue11287.pdf.link",
];

static PASSWORD: phf::Map<&'static str, &'static str> = phf::phf_map! {
    "bug1782186.pdf" => "Hello",
    "issue15893_reduced.pdf" => "test",
    "issue3371.pdf" => "ELXRTQWS",
    "issue6010_1.pdf" => "abc",
    "issue6010_2.pdf" => "\x00E6\x00F8\x00E5",
};

/// Read pdf file and render each page, to save test time,
/// touch a flag file at `$CARGO_TARGET_TMPDIR/(md5(f)).ok` if succeed.
/// If the file exist, skips the test.
///
/// If f ends with ".link", file content is a http url, download
/// that file to `$flag_file.pdf`, skip the download if `$flag_file.pdf` exists.
#[pdf_file_test_cases]
#[snafu::report]
fn render(f: &str) -> Result<()> {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        colog::init();
    });

    // return if f ends with one of IGNORED
    if IGNORED.iter().any(|s| f.ends_with(s)) {
        return Ok(());
    }

    let hash_file: String = Md5::digest(f.as_bytes()).as_slice().encode_hex();
    let mut hash_file = Path::join(Path::new(env!["CARGO_TARGET_TMPDIR"]), hash_file);
    info!("work on file: {}", f);
    hash_file.set_extension("ok");
    let hash_file = hash_file;
    if hash_file.exists() {
        info!(
            "hash file {} exist, skip this previous succeed file",
            hash_file.to_str().unwrap(),
        );
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
                let url = std::fs::read_to_string(f).whatever_context("read file")?;
                let url = url.trim();
                let mut err = Ok(());
                for url in url.lines().filter(|f| !f.is_empty()) {
                    err = download_file(url, &pdf_file);
                    if err.is_ok() {
                        break;
                    }
                }
                err?;
            }
        }
        info!("cached pdf file: {}", pdf_file.to_str().unwrap());
    }

    let buf = std::fs::read(file_path).whatever_context("read file")?;
    let file_name = Path::new(file_path).file_name().unwrap().to_str().unwrap();
    let pdf = File::parse(buf, PASSWORD.get(file_name).copied().unwrap_or(""))
        .whatever_context("open pdf file")?;
    let resolver = pdf.resolver().whatever_context("get resolver")?;
    let catalog = pdf.catalog(&resolver).whatever_context("parse catalog")?;
    for (idx, page) in catalog
        .pages()
        .whatever_context("parse pages")?
        .into_iter()
        .enumerate()
    {
        info!("Page: {}", idx);
        let option = RenderOptionBuilder::new().zoom(0.75).fail_fast(true);
        render_page(&page, option).with_whatever_context(|_| format!("render page: {}", idx))?;
    }
    std::fs::write(&hash_file, "").whatever_context("write hsah file")?;

    Ok(())
}
