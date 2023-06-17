use glob::glob;
use pdf2docx::file::File;

#[test]
fn scan_objects() {
    for entry in glob("sample_files/normal/**/*.pdf").unwrap() {
        let path = entry.unwrap();
        let buf = std::fs::read(&path).unwrap();
        println!("parsing {:?}", path);
        File::parse(&buf[..]).unwrap_or_else(|_| panic!("failed to parse {:?}", path));
    }
}
