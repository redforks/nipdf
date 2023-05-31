use std::{env::args, str::from_utf8};

use lopdf::Document;

fn dump_ver(doc: &Document) {
    println!("PDF version: {}", doc.version);
}

fn dump_trailer(doc: &Document) {
    // trailer is an index to objects(not all objects), maybe used for cross-reference
    doc.trailer.iter().for_each(|(k, v)| {
        println!("{}: {:?}", from_utf8(k).unwrap(), v);
    });
}

fn main() {
    let filename = args().nth(1).expect("Usage: dump-pdf <filename>");

    let doc = Document::load(filename).unwrap();
    dump_ver(&doc);
    dump_trailer(&doc);
}
