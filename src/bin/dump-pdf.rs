use clap::{arg, Command};
use pdf2docx::dump::{
    object::DictionaryDumper, objects::dump_objects, query::query, xref::dump_xref, ObjectType,
};

use lopdf::Document;

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
                .arg(arg!([id] "Object ID to dump"))
                .arg(arg!(-r --raw "Dump stream object content"))
                .arg(arg!(-d --decode "Decode stream object content, no effect if not set --raw option")),
        )
        .subcommand(
            Command::new("query")
                .about("Query objects")
                .long_about("Query objects, the actual behavior depensd on object type.
                
                For stream objects, queries the attached Dict")
                .visible_alias("q")
                .arg(arg!(-t --type <TYPE> "Object type to query, default is stream"))
                .arg(arg!(-i --"ignore-case" "Ignore case when both in field name and value"))
                .arg(arg!([query] "Query string, e.g. /Filter /Filter=ASCIIHexDecode /Filter*=Hex"))
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
            sub_m.get_one::<bool>("raw").copied().unwrap_or(false),
            sub_m.get_one::<bool>("decode").copied().unwrap_or(false),
        ),
        Some(("query", sub_m)) => query(
            &doc,
            sub_m
                .get_one::<String>("type")
                .and_then(|s| s.parse().ok())
                .unwrap_or(ObjectType::default()),
            sub_m.get_one::<String>("query"),
            sub_m
                .get_one::<bool>("ignore-case")
                .copied()
                .unwrap_or(false),
        ),
        _ => todo!(),
    }
}
