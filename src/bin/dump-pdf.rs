use pdf2docx::dump::object::DictionaryDumper;
use std::env::args;

use lopdf::Document;

fn dump_basic_info(doc: &Document) {
    println!("PDF Version: {}", doc.version);
    println!("Max ID: {}", doc.max_id);
    println!("Max Bookmark Id: {}", doc.max_bookmark_id);
    println!("Xref Start: {}", doc.xref_start);
}

fn dump_trailer(doc: &Document) {
    println!("\nTrailer: ");
    println!("{:}", DictionaryDumper::new(&doc.trailer));
}

fn main() {
    let filename = args().nth(1).expect("Usage: dump-pdf <filename>");

    let doc = Document::load(filename).unwrap();
    dump_basic_info(&doc);
    dump_trailer(&doc);
}
