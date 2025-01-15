use glob::glob;
use nipdf::{
    Result,
    file::File,
    object::{Object, ObjectValueError, RuntimeObjectId},
};
use snafu::{FromString as _, ResultExt as _, Whatever, report};

#[report]
#[test_log::test]
fn scan_objects() -> Result<(), Whatever> {
    for entry in glob("sample_files/normal/**/*.pdf").whatever_context("glob sample files")? {
        let path = entry.whatever_context("get file entry")?;
        let buf = std::fs::read(&path).whatever_context("read file")?;
        println!("parsing {path:?}");
        let f = File::parse(buf, "").whatever_context("open pdf file")?;
        let resolver = f.resolver().whatever_context("get resolver")?;
        for id in 1..resolver.n() {
            print!("scan object: {id}");
            match resolver.resolve(RuntimeObjectId(id.try_into().unwrap())) {
                Err(ObjectValueError::ObjectIDNotFound { .. }) => {
                    print!(" not found");
                }
                Err(e) => {
                    return Err(Whatever::with_source(
                        Box::new(e),
                        "resolve object".to_owned(),
                    ));
                }
                Ok(Object::Stream(s)) => {
                    if s.guess_is_image(&resolver) {
                        s.decode_image(&resolver, None)
                            .whatever_context("decode stream as image")?;
                    } else {
                        s.decode(&resolver).whatever_context("decode stream")?;
                    }
                }
                _ => {}
            }

            println!("  done");
        }

        for (idx, page) in f
            .catalog(&resolver)
            .whatever_context("get catalog")?
            .pages()
            .whatever_context("get pages")?
            .iter()
            .enumerate()
        {
            println!("page: {}, object id: {}", idx, page.id());
            println!("  media_box: {:?}", page.media_box());
            println!("  crop_box: {:?}", page.crop_box());

            for op in page
                .content()
                .whatever_context("get page content")?
                .operations()
                .whatever_context("parse page operations")?
            {
                println!("  {:?}", op);
            }
        }
        println!();
    }
    Ok(())
}
