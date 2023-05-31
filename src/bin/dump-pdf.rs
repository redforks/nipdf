use std::env::args;

use lopdf::Document;

fn dump_ver(doc: &Document) {
    println!("PDF version: {}", doc.version);
}

fn main() {
    let filename = args().nth(1).expect("Usage: dump-pdf <filename>");

    let doc = Document::load(filename).unwrap();
    dump_ver(&doc);
}
