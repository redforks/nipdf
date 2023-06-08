use crate::dump::{
    dump_primitive::{OptionPrimitiveDumper, OptionRectDumper},
    OptionDumper,
};

use super::FileWithXRef;

pub fn page(f: &FileWithXRef, page_no: Option<u32>) {
    if let Some(page_no) = page_no {
        let page = f.f().get_page(page_no).unwrap();
        println!("Media Box: {}", OptionRectDumper(&page.media_box));
        println!("Crop Box: {}", OptionRectDumper(&page.crop_box));
        println!("Trim Box: {}", OptionRectDumper(&page.trim_box));
        println!("Rotate: {}", page.rotate);
        println!("Metadata: {}", OptionPrimitiveDumper(&page.metadata));
        println!("lgi: {}", OptionPrimitiveDumper(&page.lgi));
        println!("vp: {}", OptionPrimitiveDumper(&page.vp));
        println!(
            "\nContents:\n{:#?}",
            OptionDumper::new(&page.contents.as_ref().map(|x| x.operations(f.f()).unwrap()))
        );
    } else {
        println!("Total pages: {}", f.f().num_pages());
    }
}
