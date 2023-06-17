use glob::glob;
use pdf2docx::{file::File, parser::parse_frame_set};

#[test]
fn scan_objects() {
    for entry in glob("sample_files/normal/**/*.pdf").unwrap() {
        let path = entry.unwrap();
        let buf = std::fs::read(&path).unwrap();
        println!("parsing {:?}", path);
        File::parse(&buf[..]).expect(&format!("failed to parse {:?}", path));
    }
}
