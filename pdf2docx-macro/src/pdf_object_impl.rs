use proc_macro::TokenStream;
use proc_macro2::Ident;
use quote::quote;
use syn::{parse_macro_input, parse_quote, Expr, ExprLit, ItemTrait, Type};

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
        }
    };
    println!("{}", tokens);
    tokens.into()
}
