use std::process::ExitCode;

use clap::{arg, Command};

fn cli() -> Command {
    Command::new("dump-pdf")
        .about("Dump PDF file structure and contents")
        .subcommand_required(true)
        // .arg(arg!(<filename> "PDF file to dump"))
        .subcommand(Command::new("hello").visible_alias("ls").about("Hello"))
}

fn hello() {
    println!("Hello, world!");
}

fn main() -> ExitCode {
    match cli().get_matches().subcommand() {
        Some(("hello", _)) => hello(),
        _ => todo!(),
    }

    ExitCode::SUCCESS
}
