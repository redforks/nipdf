use pdf2docx::{file::FrameSet, parser::parse_frame_set};
use test_case::test_case;

#[test_case("doc/PDF32000_2008.pdf")]
#[test_case("doc/pdfreference1.0.pdf")]
#[test_case("doc/SamplePdf1_12mb_6pages.pdf")]
fn scan_objects(f: &str) {
    let buf = std::fs::read(f).unwrap();
    let (_, _frames) = parse_frame_set(&buf).unwrap();
}
