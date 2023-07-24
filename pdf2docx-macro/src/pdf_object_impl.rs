use core::panic;
use std::ops::Deref;

use either::{Either, Left, Right};
use proc_macro::TokenStream;
use proc_macro2::Ident;
use quote::quote;
use syn::{
    parse_macro_input, parse_quote, Attribute, Expr, ExprLit, ExprTuple, ItemTrait, Lit, LitStr,
    ReturnType, TraitItem, TraitItemFn, Type,
};

fn snake_case_to_pascal(s: &str) -> String {
    let s = s.to_string();
    let mut chars = s.chars();
    let mut result = String::with_capacity(s.len());
    let mut first = true;
    while let Some(c) = chars.next() {
        if first {
            result.push(c.to_ascii_uppercase());
            first = false;
            continue;
        }
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

// Return left means Option<T>, right means T, Return None means not nested
fn nested<'a>(rt: &'a Type, attrs: &'a [Attribute]) -> Option<Either<&'a Type, &'a Type>> {
    if attrs.iter().any(|attr| attr.path().is_ident("nested")) {
        // check `rt` is Option<T> or T
        Some(if let Type::Path(tp) = rt {
            if let Some(seg) = tp.path.segments.last() {
                if seg.ident == "Option" {
                    Left(
                        if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                            if args.args.len() == 1 {
                                if let syn::GenericArgument::Type(ty) = &args.args[0] {
                                    ty
                                } else {
                                    panic!("expect type argument")
                                }
                            } else {
                                rt
                            }
                        } else {
                            panic!("expect angle bracketed arguments")
                        },
                    )
                } else {
                    Right(rt)
                }
            } else {
                panic!("expect path segment")
            }
        } else {
            Right(rt)
        })
    } else {
        None
    }
}

/// Return left means Option<T>, right means T, Return None means `from_name_str` attr not defined.
fn from_name_str<'a>(rt: &'a Type, attrs: &'a [Attribute]) -> Option<Either<&'a Type, &'a Type>> {
    if attrs
        .iter()
        .any(|attr| attr.path().is_ident("from_name_str"))
    {
        // check `rt` is Option<T> or T
        Some(if let Type::Path(tp) = rt {
            if let Some(seg) = tp.path.segments.last() {
                if seg.ident == "Option" {
                    Left(
                        if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                            if args.args.len() == 1 {
                                if let syn::GenericArgument::Type(ty) = &args.args[0] {
                                    ty
                                } else {
                                    panic!("expect type argument")
                                }
                            } else {
                                rt
                            }
                        } else {
                            panic!("expect angle bracketed arguments")
                        },
                    )
                } else {
                    Right(rt)
                }
            } else {
                panic!("expect path segment")
            }
        } else {
            Right(rt)
        })
    } else {
        None
    }
}

fn schema_method_name(rt: &Type, attrs: &[Attribute]) -> Option<&'static str> {
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
            Some("required_name")
        } else {
            Some("required_str")
        }
    } else if rt == &(parse_quote!(u32)) {
        Some("required_u32")
    } else if rt == &(parse_quote!(Option<u32>)) {
        Some("opt_u32")
    } else if rt == &(parse_quote!(Option<u8>)) {
        Some("opt_u8")
    } else if rt == &(parse_quote!(Option<Rectangle>)) {
        Some("opt_rect")
    } else if rt == &(parse_quote!(Rectangle)) {
        Some("required_rect")
    } else if rt == &(parse_quote!(Vec<u32>)) {
        if get_type().is_some_and(|s| s == "Ref") {
            Some("ref_id_arr")
        } else {
            Some("u32_arr")
        }
    } else if rt == &(parse_quote!(Option<&'b Dictionary<'a>>)) {
        Some("opt_dict")
    } else if rt == &(parse_quote!(&'b Dictionary<'a>)) {
        Some("required_dict")
    } else {
        None
    }
}

fn remove_generic(t: &Type) -> Type {
    if let Type::Path(tp) = t {
        let mut tp = tp.clone();
        if let Some(seg) = tp.path.segments.last_mut() {
            seg.arguments = syn::PathArguments::None;
        }
        tp.into()
    } else {
        panic!("expect path type")
    }
}

pub fn pdf_object(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr_expr = parse_macro_input!(attr as Expr);
    // println!("{:#?}", attr_expr);

    // Parse pdf_object attribute argument to (Type, Expr),
    // Type is `SchemaDict` 3rd generic parameter,
    // Expr is `SchemaDict::new()` 3rd argument.
    //
    // Attribute argument has fowling forms:
    //
    // 1. () => `((), ())`
    // 1. `&str` => `(&'static str, Expr::Lit(Lit::Str))`
    // 1. `[&str; N]` => `([&'static str; N], Expr::Array)`
    // 1. `Option<&str>` => `(Option<&'static str>, Expr::Option)`
    // 1. (Option<&str>, &str) => `(Option<&'static str>, Expr::Tuple)`
    let (valid_ty, valid_arg): (Type, Expr) = match attr_expr {
        Expr::Lit(lit) => {
            let lit = lit.lit;
            match lit {
                syn::Lit::Str(lit) => (
                    parse_quote! { &'static str },
                    Expr::Lit(ExprLit {
                        attrs: vec![],
                        lit: lit.into(),
                    }),
                ),
                _ => panic!("expect string literal"),
            }
        }

        Expr::Tuple(ExprTuple {
            attrs: _,
            paren_token: _,
            elems,
        }) if elems.is_empty() => (parse_quote!(()), parse_quote!(())),

        Expr::Tuple(ExprTuple {
            attrs: _,
            paren_token: _,
            elems,
        }) if elems.len() == 2 => {
            let (t, st) = (&elems[0], &elems[1]);
            match t {
                Expr::Path(t) if t.path.is_ident("None") => (),
                Expr::Call(c) => {
                    let func = &c.func;
                    match &func.deref() {
                        Expr::Path(path) => {
                            assert!(path.path.is_ident("Some"));
                        }
                        _ => panic!("expect path"),
                    }
                    assert_eq!(1, c.args.len());
                    assert!(matches!(
                        c.args[0],
                        Expr::Lit(ExprLit {
                            lit: Lit::Str(_),
                            attrs: _
                        })
                    ));
                }
                _ => panic!("expect path"),
            }
            assert!(matches!(
                st,
                Expr::Lit(ExprLit {
                    lit: Lit::Str(_),
                    attrs: _
                })
            ));
            (
                parse_quote! { (Option<&'static str>, &'static str)},
                parse_quote!( (#t, #st)),
            )
        }

        Expr::Array(arr) => {
            let mut ty: Vec<Type> = vec![];
            let mut arg = vec![];
            for expr in arr.elems {
                match expr {
                    Expr::Lit(lit) => {
                        let lit = lit.lit;
                        match lit {
                            syn::Lit::Str(lit) => {
                                ty.push(parse_quote! { &'static str });
                                arg.push(Expr::Lit(ExprLit {
                                    attrs: vec![],
                                    lit: lit.into(),
                                }));
                            }
                            _ => panic!("expect string literal"),
                        }
                    }
                    _ => panic!("expect string literal"),
                }
            }
            let len = ty.len();
            (
                parse_quote! { [&'static str; #len] },
                Expr::Array(parse_quote!([ #(#arg),* ])),
            )
        }

        Expr::Call(ref call) => {
            let func = &call.func;
            match &func.deref() {
                Expr::Path(path) => {
                    assert!(path.path.is_ident("Some"));
                    (parse_quote! { Option<&'static str>}, attr_expr)
                }
                _ => panic!("expect path"),
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
                    ReturnType::Type(_, ty) => ty,
                };
                let key = snake_case_to_pascal(&name.to_string());
                let method = schema_method_name(rt, &attrs[..]).map(|m| Ident::new(m, name.span()));
                if let Some(method) = method {
                    methods.push(quote! {
                        fn #name(&self) -> #rt {
                            self.d.#method(#key).unwrap()
                        }
                    });
                } else if let Some(nested_type) = nested(rt, attrs) {
                    let type_name = remove_generic(&nested_type);
                    match nested_type {
                        Left(ty) => {
                            methods.push(quote! {
                                fn #name(&self) -> Option<#ty> {
                                    self.d.opt_dict(#key).unwrap().map(|d| #type_name::new(d).unwrap())
                                }
                            });
                        }
                        Right(ty) => {
                            methods.push(quote! {
                                fn #name(&self) -> #ty {
                                    #type_name::new(self.d.required_dict(#key).unwrap()).unwrap()
                                }
                            });
                        }
                    }
                } else if let Some(from_name_str_type) = from_name_str(rt, attrs) {
                    // let type_name = remove_generic(&from_name_str_type);
                    match from_name_str_type {
                        Left(ty) => {
                            methods.push(quote! {
                                fn #name(&self) -> Option<#ty> {
                                    self.d.opt_name(#key).unwrap().map(|s| <#ty as std::str::FromStr>::from_str(s).unwrap())
                                }
                            });
                        }
                        Right(ty) => {
                            methods.push(quote! {
                                fn #name(&self) -> #ty {
                                    <#ty as std::str::FromStr>::from_str(
                                        self.d.required_name(#key).unwrap()
                                    ).unwrap()
                                }
                            });
                        }
                    }
                } else {
                    panic!("unsupported return type")
                }
            }
            _ => panic!("only support function"),
        }
    }

    let tokens = quote! {
        struct #struct_name<'a, 'b> {
            d: SchemaDict<'a, 'b, #valid_ty>,
        }

        impl<'a, 'b> #struct_name<'a, 'b> {
            fn new(dict: &'b Dictionary<'a>) -> Result<Self, ObjectValueError> {
                let d = SchemaDict::new(dict, #valid_arg)?;
                Ok(Self { d })
            }

            fn from(dict: &'b Dictionary<'a>) -> Result<Option<Self>, ObjectValueError> {
                let d = SchemaDict::from(dict, #valid_arg)?;
                Ok(d.map(|d| Self { d }))
            }

            #(#methods)*
        }
    };
    // println!("{}", tokens);
    tokens.into()
}
