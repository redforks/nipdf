use std::num::NonZeroU32;

use glob::glob;
use nipdf::{
    file::{File, ObjectResolver},
    object::{Object, ObjectValueError},
};

#[test_log::test]
fn scan_objects() {
    for entry in glob("sample_files/normal/**/*.pdf").unwrap() {
        let path = entry.unwrap();
        let buf = std::fs::read(&path).unwrap();
        println!("parsing {path:?}");
        let (f, xref) =
            File::parse(&buf[..]).unwrap_or_else(|_| panic!("failed to parse {path:?}"));
        let resolver = ObjectResolver::new(&buf[..], &xref);
        for id in 1..f.total_objects() {
            print!("scan object: {id}");
            match resolver.resolve(NonZeroU32::new(id).unwrap()) {
                Err(ObjectValueError::ObjectIDNotFound(_)) => {
                    print!(" not found")
                }
                Err(e) => panic!("{}", e),
                Ok(Object::Stream(s)) => s
                    .decode(&resolver)
                    .map(|_| ())
                    .or_else(|_| s.decode_image(&resolver, None).map(|_| ()))
                    .unwrap(),
                _ => {}
            }

            println!("  done");
        }

        for (idx, page) in f
            .catalog(&resolver)
            .unwrap()
            .pages()
            .unwrap()
            .iter()
            .enumerate()
        {
            println!("page: {}, object id: {}", idx, page.id());
            println!("  media_box: {:?}", page.media_box());
            println!("  crop_box: {:?}", page.crop_box());

            for op in page.content().unwrap().operations() {
                println!("  {:?}", op);
            }
        }
        println!();
    }
}
