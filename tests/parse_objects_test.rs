use glob::glob;
use pdf2docx::file::File;

#[test]
fn scan_objects() {
    for entry in glob("sample_files/normal/**/*.pdf").unwrap() {
        let path = entry.unwrap();
        let buf = std::fs::read(&path).unwrap();
        println!("parsing {:?}", path);
        let (f, mut resolver) =
            File::parse(&buf[..]).unwrap_or_else(|_| panic!("failed to parse {:?}", path));
        for id in 0..f.total_objects {
            print!("scan object: {}", id);
            let obj = resolver.resolve(id);
            if obj.is_none() {
                println!("  missing");
            } else {
                println!("  done");
            }
        }
    }
}
