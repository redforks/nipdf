use glob::glob;
use itertools::Itertools;
use proc_macro::TokenStream;
use quote::quote;
use std::{
    env::var,
    path::{Path, PathBuf},
};

/// Glob `*.pdf`, `*.pdf.link` files in `sample_files`, `../pdf/`, `pdf.js/test/pdfs` directories,
/// relative to crate directory.
/// for each file generate a test, using `#[test-case]` attribute.
/// For example: `#[test_case("sample_files/normal/foo.pdf")]`
///
/// To save compile time, file list cached in `${workspace}/target/render-test.list` file, if file
/// not exist, it will re-generated by directories. Each line in cache file is a file path.
///
/// Using `proc-macro2`, `syn`, `quote` crates to help for parsing and generating code.
#[proc_macro_attribute]
pub fn pdf_file_test_cases(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let cache_file = Path::new(&var("CARGO_TARGET_TMPDIR").unwrap()).join("render-test.list");
    let files = if cache_file.exists() {
        let cache = std::fs::read_to_string(cache_file).unwrap();
        cache
            .lines()
            .filter(|&l| (!l.is_empty())).map(|l| l.to_owned())
            .collect()
    } else {
        let dirs = vec!["sample_files", "../../pdf", "pdf.js/test/pdfs"];
        let patterns = vec!["**/*.pdf", "**/*.pdf.link"];
        let files = dirs
            .into_iter()
            .cartesian_product(patterns)
            .flat_map(|(dir, pattern)| {
                let dir: PathBuf = [&var("CARGO_MANIFEST_DIR").unwrap(), dir, pattern]
                    .iter()
                    .collect();
                glob(dir.to_str().unwrap())
                    .unwrap()
                    .map(|p| p.unwrap().to_str().unwrap().to_owned())
            })
            .collect_vec();
        std::fs::write(cache_file, files.join("\n")).unwrap();
        files
    };

    let mut test_case_attrs = Vec::with_capacity(files.len());
    for file in files {
        let test_case_attr = quote! {
            #[test_case(#file)]
        };
        test_case_attrs.push(test_case_attr);
    }

    let input = syn::parse_macro_input!(item as syn::ItemFn);
    let tokens = quote! {
        #(#test_case_attrs)*
        #input
    };
    tokens.into()
}
