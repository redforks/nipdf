use clap::{arg, Command};
use pdf2docx::dump::object::DictionaryDumper;
use pdf2docx::dump::xref::XrefDumper;

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
}

fn main() {
    let matches = cli().get_matches();
    let filename: &String = matches.get_one("filename").unwrap();
    let doc = Document::load(filename).unwrap();

    match cli().get_matches().subcommand() {
        Some(("summary", _)) => {
            dump_basic_info(&doc);
            dump_trailer(&doc);
            println!("\n{}", XrefDumper::new(&doc.reference_table));
        }
        _ => todo!(),
    }
}
