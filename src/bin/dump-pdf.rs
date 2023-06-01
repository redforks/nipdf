use clap::{arg, Command};
use pdf2docx::dump::{object::DictionaryDumper, objects::dump_objects, xref::dump_xref};

use lopdf::{Document, Object, ObjectId};

fn summary(doc: &Document) {
    println!("PDF Version: {}", doc.version);
    println!("Max ID: {}", doc.max_id);
    println!("Max Bookmark Id: {}", doc.max_bookmark_id);
    println!("Xref Start: {}", doc.xref_start);

    println!("\nTrailer: ");
    println!("{:}", DictionaryDumper::new(&doc.trailer));

    println!("\nxref:");
    println!("type: {:?}", doc.reference_table.cross_reference_type);
    println!("size: {}", doc.reference_table.size);
}

fn cli() -> Command {
    Command::new("dump-pdf")
        .about("Dump PDF file structure and contents")
        .subcommand_required(true)
        .arg(arg!(<filename> "PDF file to dump"))
        .subcommand(
            Command::new("summary")
                .visible_alias("ls")
                .about("Dump PDF file summary"),
        )
        .subcommand(
            Command::new("xref")
                .about("Dump xref table")
                .arg(arg!([id] "Object ID to dump")),
        )
        .subcommand(
            Command::new("objects")
                .about("Dump objects")
                .arg(arg!([id] "Object ID to dump")),
        )
}

fn main() {
    let matches = cli().get_matches();
    let filename: &String = matches.get_one("filename").unwrap();
    let doc = Document::load(filename).unwrap();

    match cli().get_matches().subcommand() {
        Some(("summary", _)) => summary(&doc),
        Some(("xref", sub_m)) => dump_xref(
            &doc,
            sub_m.get_one::<String>("id").map(|s| s.parse().unwrap()),
        ),
        Some(("objects", sub_m)) => dump_objects(
            &doc,
            sub_m.get_one::<String>("id").and_then(|s| s.parse().ok()),
        ),
        _ => todo!(),
    }
}
