use std::env::args;

fn main() {
    let filename = args().nth(1).expect("Usage: dump-pdf <filename>");
    println!("{}", filename);
}
