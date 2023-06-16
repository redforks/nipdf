use glob::glob;
use pdf2docx::parser::parse_frame_set;

#[test]
fn scan_objects() {
    for entry in glob("doc/*.pdf").unwrap() {
        let path = entry.unwrap();
        let buf = std::fs::read(&path).unwrap();
        let (_, _frames) = parse_frame_set(&buf)
            .unwrap_or_else(|_| panic!("{}", path.to_str().unwrap().to_string()));
    }
}
