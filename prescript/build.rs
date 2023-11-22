//! Build script that convert `names` file located at workspace root directory,
//! to src/name/built_in_names.rs.
//!
//! Each line in `names` file is a PostScript name, line start with '#' is a comment,
//! empty line is ignored.
//! Sort the names, and generate the builtin_in_names.rs file

use std::{fs::read_to_string, io::Write, path::Path};

fn main() {
    let names_file = Path::join(Path::new(env!("CARGO_MANIFEST_DIR")), "../names");
    let dest_file = Path::join(
        Path::new(env!("CARGO_MANIFEST_DIR")),
        "src/name/built_in_names.rs",
    );

    // return if names_file is not newer than dest_file
    if let Ok(names_meta) = std::fs::metadata(&names_file) {
        if let Ok(dest_meta) = std::fs::metadata(&dest_file) {
            if names_meta.modified().unwrap() <= dest_meta.modified().unwrap() {
                return;
            }
        }
    }

    let names = read_to_string(names_file).unwrap();
    let mut names: Vec<_> = names
        .lines()
        .map(|s| s.trim())
        .filter(|w| !w.is_empty() && !w.starts_with('#'))
        .collect();
    names.sort_unstable();
    names.dedup();
    let mut file = std::fs::File::create(dest_file).unwrap();
    let n = names.len();
    writeln!(file, "pub(crate) static BUILT_IN_NAMES: [&str; {n}] = [").unwrap();
    for name in names {
        writeln!(file, "    {:?},", name).unwrap();
    }
    writeln!(file, "];").unwrap();
}
