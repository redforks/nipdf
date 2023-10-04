use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, parse_quote, ImplItemFn, ReturnType};

/// proc macro function to wrap current method that returns `anyhow::Result<()>`,
/// into a new method returns `()`, if the inner method returns `Err`, it will
/// save the error into current struct field `err`, and return `()`.
pub fn save_error(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut method = parse_macro_input!(item as ImplItemFn);
    // check method return type is `Result<()>`
    fn ensure_return_type(rt: &ReturnType) {
        if let ReturnType::Type(_, ty) = rt {
            if ty == &(parse_quote!(Result<()>)) {
                return;
            }
        }

        panic!("method must return Result<()>");
    }
    ensure_return_type(&method.sig.output);

    // check first argument is `&mut self`
    if method.sig.inputs.len() == 0 {
        panic!("method must have at least one argument")
    }
    let first_arg = &method.sig.inputs[0];
    if let syn::FnArg::Receiver(receiver) = first_arg {
        if receiver.reference.is_none() || receiver.mutability.is_none() {
            panic!("first argument must be &mut self")
        }
    } else {
        panic!("first argument must be &mut self")
    }

    // remove method return type
    method.sig.output = ReturnType::Default;

    let body = method.block;
    method.block = parse_quote! {
    {
        let _return_ = (|| -> Result<()> #body)();
        match _return_ {
            Ok(()) => {
                self.err = None;
            },
            Err(e) => {
                self.error(e);
            }
        }
    }
    };
    let tokens = quote!(#method);
    // println!("{}", tokens);
    tokens.into()
}
