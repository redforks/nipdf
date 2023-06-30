use glob::glob;
use pdf2docx::{
    file::File,
    object::{Object, ObjectValueError},
};

#[test_log::test]
fn scan_objects() {
    for entry in glob("sample_files/normal/**/*.pdf").unwrap() {
        let path = entry.unwrap();
        let buf = std::fs::read(&path).unwrap();
        println!("parsing {:?}", path);
        let (f, mut resolver) =
            File::parse(&buf[..]).unwrap_or_else(|_| panic!("failed to parse {:?}", path));
        for id in 0..f.total_objects {
            print!("scan object: {}", id);
            match resolver.resolve(id) {
                Err(ObjectValueError::ObjectIDNotFound) => {
                    print!(" not found")
                }
                Err(e) => panic!("{}", e),
                Ok(Object::Stream(s)) => {
                    s.decode().unwrap();
                }
                _ => {}
            }

            println!("  done");
        }
    }
}
