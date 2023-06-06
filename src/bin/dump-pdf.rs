use std::{process::ExitCode, sync::Arc};

use clap::{arg, Command};
use pdf::{
    any::AnySync,
    file::{Cache, File},
    object::{NoUpdate, ObjNr, PlainRef, Resolve, ToDict},
    PdfError,
};
use pdf2docx::dump::dump_primitive::{DictionaryDumper, PrimitiveDumper};
use pdf2docx::dump::FileWithXRef;

fn trailer<OC, SC>(f: &File<Vec<u8>, OC, SC>) {
    println!(
        "Trailer:\n{}",
        DictionaryDumper::new(&f.trailer.to_dict(&mut NoUpdate).unwrap())
    );
}

fn catalog<OC, SC>(f: &File<Vec<u8>, OC, SC>)
where
    OC: Cache<Result<AnySync, Arc<PdfError>>>,
    SC: Cache<Result<Arc<[u8]>, Arc<PdfError>>>,
{
    println!(
        "Catalog:\n{}",
        DictionaryDumper::new(&f.get_root().to_dict(&mut NoUpdate).unwrap())
    );
}

fn object_ids(f: &FileWithXRef) {
    let table = f.xref_table();
    for id in table.iter() {
        if let Ok(xref) = table.get(id as ObjNr) {
            print!("{}: ", id);
            let plain_ref = PlainRef {
                id: id as ObjNr,
                gen: xref.get_gen_nr(),
            };
            if let Ok(obj) = f.f().resolve(plain_ref) {
                println!(
                    "{}: {}",
                    PrimitiveDumper::new(&plain_ref.into()),
                    PrimitiveDumper::new(&obj)
                );
            }
        }
    }
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
    let f = FileWithXRef::open(filename);

    match cli().get_matches().subcommand() {
        Some(("trailer", _)) => trailer(f.f()),
        Some(("catalog", _)) => catalog(f.f()),
        Some(("objects", sub_m)) => object_ids(&f),
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
