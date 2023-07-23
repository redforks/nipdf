use proc_macro::TokenStream;
use proc_macro2::Ident;
use quote::quote;
use syn::{
    parse_macro_input, parse_quote, Attribute, Expr, ExprLit, ItemTrait, Lit, LitStr, ReturnType,
    TraitItem, TraitItemFn, Type, TypeReference,
};

fn snake_case_to_pascal(s: &str) -> String {
    let mut s = s.to_string();
    let mut chars = s.chars();
    let mut result = String::new();
    while let Some(c) = chars.next() {
        if c == '_' {
            if let Some(c) = chars.next() {
                result.push(c.to_ascii_uppercase());
            }
        } else {
            result.push(c);
        }
    }
    result
}

fn schema_method_name(rt: &Type, attrs: &[Attribute]) -> &'static str {
    let get_type = || {
        attrs.iter().find_map(|attr| {
            if attr.path().is_ident("typ") {
                let lit: LitStr = attr.parse_args().expect("expect string literal");
                Some(lit.value())
            } else {
                None
            }
        })
    };
    if rt == &(parse_quote! { &str }) {
        if get_type().is_some_and(|s| s == "Name") {
            "required_name"
        } else {
            "required_str"
        }
    } else if rt == &(parse_quote!(u32)) {
        "required_u32"
    } else if rt == &(parse_quote!(Option<u32>)) {
        "opt_u32"
    } else if rt == &(parse_quote!(Option<Rectangle>)) {
        "opt_rect"
    } else if rt == &(parse_quote!(Rectangle)) {
        "required_rect"
    } else if rt == &(parse_quote!(Vec<u32>)) {
        "u32_arr"
    } else {
        todo!()
    }
}

pub fn pdf_object(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr_expr = parse_macro_input!(attr as Expr);
    println!("{:#?}", attr_expr);

    // Parse pdf_object attribute argument to (Type, Expr),
    // Type is `SchemaDict` 3rd generic parameter,
    // Expr is `SchemaDict::new()` 3rd argument.
    //
    // Attribute argument has three forms:
    //
    // 1. `&str` => `(&'static str, Expr::Lit(Lit::Str))`
    // 1. `[&str; N]` => `([&'static str; N], Expr::Array)`
    // 1. `Option<&str>` => `(Option<&'static str>, Expr::Option)`
    let (valid_ty, valid_arg) = match attr_expr {
        Expr::Lit(lit) => {
            let lit = lit.lit;
            match lit {
                syn::Lit::Str(lit) => {
                    let ty: Type = parse_quote! { &'static str };
                    (
                        ty,
                        Expr::Lit(ExprLit {
                            attrs: vec![],
                            lit: lit.into(),
                        }),
                    )
                }
                _ => panic!("expect string literal"),
            }
        }
        _ => todo!(),
    };

    let def = parse_macro_input!(item as ItemTrait);
    let name = def.ident.to_string();
    assert!(name.ends_with("Trait"));
    let struct_name = &name[..name.len() - 5];
    let struct_name = Ident::new(struct_name, def.ident.span());

    let mut methods = vec![];
    for item in &def.items {
        match item {
            TraitItem::Fn(TraitItemFn { sig, attrs, .. }) => {
                let name = sig.ident.clone();
                let rt: &Type = match &sig.output {
                    ReturnType::Default => panic!("function must have return type"),
                    ReturnType::Type(_, ty) => &ty,
                };
                let key = snake_case_to_pascal(&name.to_string());
                let method = Ident::new(schema_method_name(rt, &attrs[..]), name.span());

                methods.push(quote! {
                    fn #name(&self) -> #rt {
                        self.d.#method(#key).unwrap()
                    }
                });
            }
            _ => panic!("only support function"),
        }
    }

    let tokens = quote! {
        struct #struct_name<'a, 'b> {
            d: SchemaDict<'a, 'b, #valid_ty>,
        }

        impl<'a, 'b> #struct_name<'a, 'b> {
            fn new(id: u32, dict: &'b Dictionary<'a>) -> Result<Self, ObjectValueError> {
                let d = SchemaDict::new(id, dict, #valid_arg)?;
                Ok(Self { d })
            }

            fn id(&self) -> u32 {
                self.d.id()
            }

            #(#methods)*
        }
    };
    println!("{}", tokens);
    tokens.into()
}
