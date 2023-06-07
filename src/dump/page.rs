use super::FileWithXRef;

pub fn page(f: &FileWithXRef, page_no: Option<usize>) {
    if let Some(_page_no) = page_no {
        todo!("detail page dump not implemented")
    } else {
        println!("Total pages: {}", f.f().num_pages());
    }
}
