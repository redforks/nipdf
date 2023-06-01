use pdf2docx::dump::object::ObjectDumper;
use std::{env::args, str::from_utf8};

use lopdf::Document;

fn dump_basic_info(doc: &Document) {
    println!("PDF Version: {}", doc.version);
    println!("Max ID: {}", doc.max_id);
    println!("Max Bookmark Id: {}", doc.max_bookmark_id);
    println!("Xref Start: {}", doc.xref_start);
}

fn dump_trailer(doc: &Document) {
    println!("\nTrailer: ");
    // trailer is an index to objects(not all objects), maybe used for cross-reference
    doc.trailer.iter().for_each(|(k, v)| {
        println!("{}: {:}", from_utf8(k).unwrap(), ObjectDumper(v));
    });
}

fn main() {
    let filename = args().nth(1).expect("Usage: dump-pdf <filename>");

    let doc = Document::load(filename).unwrap();
    dump_basic_info(&doc);
    dump_trailer(&doc);
}
