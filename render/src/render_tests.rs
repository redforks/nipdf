//! Test page render result using `insta` to ensure that the rendering result is not changed.
//! This file checks file pdfreference1.0.pdf
use crate::{RenderOptionBuilder, render_page};
use anyhow::Result as AnyResult;
use insta::assert_ron_snapshot;
use md5::{Digest, Md5};
use nipdf::file::File;

/// Open file for testing. `file_path` relate to current crate directory.
fn open_test_file(file_path: impl AsRef<std::path::Path>) -> File {
    let file_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../nipdf")
        .join(file_path);
    let data = std::fs::read(file_path).unwrap();
    File::parse(data, "").unwrap()
}

fn decode_file_page(path: &str, page_no: usize) -> AnyResult<String> {
    let f = open_test_file(path);
    let resolver = f.resolver()?;
    let catalog = f.catalog(&resolver)?;
    let pages = catalog.pages()?;
    let page = &pages[page_no];
    let option = RenderOptionBuilder::new().zoom(1.5);
    let bytes = render_page(page, option)?.into_vec();
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

#[test]
fn form_ctm() {
    assert_ron_snapshot!(
        &decode_file_page("../../pdf/ICEpower125ASX2_Datasheet_2.0.pdf", 4).unwrap()
    );
}

#[test]
fn radial_shade() {
    assert_ron_snapshot!(
        &decode_file_page("sample_files/bizarre/pdfReferenceUpdated.pdf", 809).unwrap()
    )
}

#[test]
fn axial_shade() {
    // TODO: find a sample page contains PaintShading("axial-shade") operation
    assert_ron_snapshot!(&decode_file_page("../../pdf/code.pdf", 619).unwrap())
}

#[test]
fn type0_cid_font() {
    assert_ron_snapshot!(
        &decode_file_page("sample_files/bizarre/pdfReferenceUpdated.pdf", 1013).unwrap()
    )
}

#[test]
fn standard_14_font_alias_name() {
    // Font name: TimesNewRomanPSMT alias of Times-Roman, see normalize_font_name()
    assert_ron_snapshot!(&decode_file_page("../../pdf/code.pdf", 620).unwrap())
}

#[test]
fn image_mask_cal_rgb_index_color_space() {
    // test paint image has mask, and its color space is Indexed to CalRGB,
    // image stream processed with Predicator
    assert_ron_snapshot!(&decode_file_page("sample_files/filters/predictor.pdf", 0).unwrap())
}

#[test]
fn decrypt_aes_revion3() {
    assert_ron_snapshot!(&decode_file_page("../../pdf/5176.CFF.pdf", 0).unwrap());
}

#[test]
fn ttf_font_cmap_trimmed_table_mapping() {
    // font used in graph, its cmap table uses format TrimmedTableMapping
    // that ttf-parser glyph_index() don't work, see `TTFParserFontOp::char_to_gid()`
    assert_ron_snapshot!(
        &decode_file_page("pdf.js/web/compressed.tracemonkey-pldi-09.pdf", 9).unwrap()
    )
}

#[test]
fn axial_shade_with_sample_function() {
    assert_ron_snapshot!(
        &decode_file_page("pdf.js/web/compressed.tracemonkey-pldi-09.pdf", 10).unwrap()
    )
}

#[test]
fn todo_rotate_n_encrypt_alg2() {
    // page rotate
    // encrypt algorithm 2 (Algorithm::Key40AndMore)
    // todo: ForceBold font flag should render glyph bolder
    assert_ron_snapshot!(&decode_file_page("../../pdf/avr-1507-owners-manual-en.pdf", 10).unwrap())
}

#[test]
fn todo_radius_pattern() {
    // todo: tiny_skia not support end radius, the page has both start and end radius rendered
    // incorrectly
    assert_ron_snapshot!(&decode_file_page("sample_files/bizarre/PDF32000_2008.pdf", 745).unwrap())
}

#[test]
fn todo_glyph_encoding_problem() {
    // todo: incorrect bullet glyph rendered, possible because ttf-parser glyph_index() returned
    // wrong glyph index
    assert_ron_snapshot!(&decode_file_page("sample_files/bizarre/PDF32000_2008.pdf", 159).unwrap())
}

#[test]
fn todo_radius_patten_without_extension() {
    // todo: tiny_skia shader, RadialGradient and LinearGradient
    // SpreadMode no support of no extension, apply a mask can fix, but complex
    //
    // and tests background color of shading dict
    assert_ron_snapshot!(&decode_file_page("sample_files/bizarre/PDF32000_2008.pdf", 746).unwrap())
}

#[test]
fn todo_coons_patch_mesh_shading() {
    // ShadingType::CoonsPatchMesh not implemented
    assert_ron_snapshot!(&decode_file_page("sample_files/bizarre/PDF32000_2008.pdf", 747).unwrap())
}

#[test]
fn todo_interactive_form() {
    // Tests:
    //   1. media_box left-lower point not (0,0)
    //   1. multiple page content stream, and operands and operator cross streams
    //   1. todo interactive form

    assert_ron_snapshot!(&decode_file_page("pdf.js/test/pdfs/160F-2019.pdf", 0).unwrap())
}

#[test]
fn tile_pattern_with_very_large_b_box() {
    assert_ron_snapshot!(
        &decode_file_page("pdf.js/web/compressed.tracemonkey-pldi-09.pdf", 12).unwrap()
    )
}

#[test]
fn transparent() {
    assert_ron_snapshot!(&decode_file_page("pdf.js/test/pdfs/alphatrans.pdf", 0).unwrap())
}

#[test]
fn type3_font() {
    assert_ron_snapshot!(&decode_file_page("pdf.js/test/pdfs/bug1001080.pdf", 0).unwrap())
}

#[test]
fn type3_with_nagative_font_size() {
    assert_ron_snapshot!(&decode_file_page("pdf.js/test/pdfs/bug1011159.pdf", 0).unwrap())
}

#[test]
fn todo_tensor_product_patch_mesh_shading() {
    // also test:
    //   1. PostScript function(Function Type4)
    //   1. DeviceN color space
    // todo: ShadingType::TensorProductPatchMesh, the electronic header bucket shader not rendered
    assert_ron_snapshot!(
        &decode_file_page("pdf.js/test/pdfs/bug1703683_page2_reduced.pdf", 0).unwrap()
    )
}

#[test]
fn text_clip_path() {
    assert_ron_snapshot!(&decode_file_page("sample_files/xobject/text-clip.pdf", 0).unwrap())
}

#[test]
fn type1_font_units_per_em_not_1000() {
    assert_ron_snapshot!(
        &decode_file_page("../render/src/type1-units-per-em-not-1000.pdf", 0).unwrap()
    )
}
