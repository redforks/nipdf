//! Defines a function like macro `name!()` to return
//! `prescript::Name` at compile time.
use proc_macro::TokenStream;
use syn::parse_macro_input;

mod built_in_names;

/// Take a `&'static str` literal and return `prescript::Name`.
///
/// `prescript::name()` function doing a binary search to locate
/// possible builtin static names, use this macro removes the cost
/// of binary search at runtime.
#[proc_macro]
pub fn name(item: TokenStream) -> TokenStream {
    let item = parse_macro_input!(item as syn::LitStr);
    let s = item.value();
    match built_in_names::BUILT_IN_NAMES.binary_search(&&*s) {
        Ok(i) => {
            let i = i as u16;
            let tokens = quote::quote! {
                prescript::Name(either::Either::Left(#i))
            };
            tokens.into()
        }
        Err(_) => {
            eprintln!("unknown static name: {}", &s);
            let tokens = quote::quote! {
                prescript::Name(either::Either::Right(#s.into()))
            };
            tokens.into()
        }
    }
}
