use std::process::ExitCode;

use clap::{arg, Command};
use pdf::{
    file::{File, FileOptions},
    object::{NoUpdate, ToDict},
};
use pdf2docx::dump::dump_primitive::DictionaryDumper;

fn trailer<OC, SC>(f: &File<Vec<u8>, OC, SC>) {
    println!(
        "Trailer:\n{}",
        DictionaryDumper::new(&f.trailer.to_dict(&mut NoUpdate).unwrap())
    );
}

fn catalog<OC, SC>(f: &File<Vec<u8>, OC, SC>) {
    let catalog = &f.trailer.root;
    println!(
        "Catalog:\n{}",
        DictionaryDumper::new(&catalog.to_dict(&mut NoUpdate).unwrap())
    );
}

fn cli() -> Command {
    Command::new("dump-pdf")
        .about("Dump PDF file structure and contents")
        .subcommand_required(true)
        .arg(arg!(<filename> "PDF file to dump"))
        .subcommand(
            Command::new("trailer")
                .visible_alias("ls")
                .about("Dump PDF file summary"),
        )
        .subcommand(
            Command::new("catalog")
            .about("Dump catalog")
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
                .long_about("If <query> not starts with '/', search <query> at everywhere, including
Dictionary key, non-string values are converted to string and then searched.

/Filter: search for object contains key 'Filter'
/Filter=ASCIIHexDecode: search for object contains key 'Filter' and value is 'ASCIIHexDecode'
/Filter*=Hex: search for object contains key 'Filter' and value contains 'Hex'
                ")
                .visible_alias("q")
                .arg(arg!(-i --"ignore-case" "Ignore case when both in field name and value"))
                .arg(arg!(<query> "Query string, e.g. foo /Filter /Filter=ASCIIHexDecode /Filter*=Hex"))
        )
}

fn main() -> ExitCode {
    let matches = cli().get_matches();
    let filename: &String = matches.get_one("filename").unwrap();
    let f = FileOptions::uncached().open(filename).unwrap();

    match cli().get_matches().subcommand() {
        Some(("summary", _)) => trailer(&f),
        Some(("catalog", _)) => catalog(&f),
        // Some(("xref", sub_m)) => dump_xref(
        //     &doc,
        //     sub_m.get_one::<String>("id").map(|s| s.parse().unwrap()),
        // ),
        // Some(("objects", sub_m)) => dump_objects(
        //     &doc,
        //     sub_m.get_one::<String>("id").and_then(|s| s.parse().ok()),
        //     sub_m.get_one::<bool>("raw").copied().unwrap_or(false),
        //     sub_m.get_one::<bool>("decode").copied().unwrap_or(false),
        // ),
        // Some(("query", sub_m)) => {
        //     if query(
        //         &doc,
        //         sub_m.get_one::<String>("query"),
        //         sub_m
        //             .get_one::<bool>("ignore-case")
        //             .copied()
        //             .unwrap_or(false),
        //     ) {
        //         return ExitCode::SUCCESS;
        //     } else {
        //         return ExitCode::FAILURE;
        //     }
        // }
        _ => todo!(),
    }

    ExitCode::SUCCESS
}
