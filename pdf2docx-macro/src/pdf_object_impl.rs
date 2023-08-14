use core::panic;

use either::{Either, Left, Right};
use proc_macro::TokenStream;
use proc_macro2::Ident;
use quote::quote;
use syn::{
    parse_macro_input, parse_quote, Attribute, Expr, ExprCall, ExprLit, ExprTuple, ItemTrait, Lit,
    LitStr, ReturnType, TraitItem, TraitItemFn, Type,
};

/// If `#[key("key")]` attribute defined, return key value
fn key_attr(attrs: &[Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| {
        if attr.path().is_ident("key") {
            let lit: LitStr = attr.parse_args().expect("expect string literal");
            Some(lit.value())
        } else {
            None
        }
    })
}

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

/// Get type from `Option<T>`
fn unwrap_option_type(t: &Type) -> &Type {
    if let Type::Path(tp) = t {
        if let Some(seg) = tp.path.segments.last() {
            if seg.ident == "Option" {
                if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                    assert_eq!(1, args.args.len());
                    if let syn::GenericArgument::Type(ty) = &args.args[0] {
                        return ty;
                    } else {
                        panic!("expect type argument")
                    };
                }
            }
        }
    }
    panic!("expect Option<T>")
}

fn has_attr<'a>(
    attr_name: &str,
    rt: &'a Type,
    attrs: &'a [Attribute],
) -> Option<Either<&'a Type, &'a Type>> {
    if attrs.iter().all(|attr| !attr.path().is_ident(attr_name)) {
        return None;
    }

    let Type::Path(tp) = rt else {
        return Some(Right(rt));
    };

    let Some(seg) = tp.path.segments.last()  else {
        panic!("expect path segment")
    };

    if seg.ident != "Option" {
        return Some(Right(rt));
    }

    Some(Left(rt))
}

fn _is_type(t: &Type, type_name: &'static str) -> bool {
    if let Type::Path(tp) = t {
        if let Some(seg) = tp.path.segments.last() {
            return seg.ident == type_name;
        }
    }

    false
}

fn is_vec(t: &Type) -> bool {
    _is_type(t, "Vec")
}

fn is_map(t: &Type) -> bool {
    _is_type(t, "HashMap")
}

/// Return Some(literal) if `#[default(literal)]` attribute defined, otherwise return None
fn default_lit(attrs: &[Attribute]) -> Option<ExprLit> {
    attrs.iter().find_map(|attr| {
        if attr.path().is_ident("default") {
            let lit: ExprLit = attr.parse_args().expect("expect literal");
            Some(lit)
        } else {
            None
        }
    })
}

/// Return Some(func_name) if `#[default_fn("func_name")]` attribute defined, otherwise return None
fn default_fn(attrs: &[Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| {
        if attr.path().is_ident("default_fn") {
            let lit: LitStr = attr.parse_args().expect("expect string literal");
            Some(lit.value())
        } else {
            None
        }
    })
}

/// Return true if `#[or_default]` attribute defined.
fn or_default(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| attr.path().is_ident("or_default"))
}

enum DefaultAttr {
    Literal(ExprLit),
    Function(String),
    OrDefault,
}

fn parse_default_attr(attrs: &[Attribute]) -> Option<DefaultAttr> {
    if let Some(lit) = default_lit(attrs) {
        Some(DefaultAttr::Literal(lit))
    } else if or_default(attrs) {
        Some(DefaultAttr::OrDefault)
    } else {
        default_fn(attrs).map(DefaultAttr::Function)
    }
}

// Return left means Option<T>, right means T, Return None means not nested
fn nested<'a>(rt: &'a Type, attrs: &'a [Attribute]) -> Option<Either<&'a Type, &'a Type>> {
    has_attr("nested", rt, attrs)
}

/// Return left means Option<T>, right means T, Return None means `from_name_str` attr not defined.
fn from_name_str<'a>(rt: &'a Type, attrs: &'a [Attribute]) -> Option<Either<&'a Type, &'a Type>> {
    has_attr("from_name_str", rt, attrs)
}

/// Return left means Option<T>, right means T, Return None means `try_from` attr not defined.
fn try_from<'a>(rt: &'a Type, attrs: &'a [Attribute]) -> Option<Either<&'a Type, &'a Type>> {
    has_attr("try_from", rt, attrs)
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
    } else if rt == &(parse_quote!(Option<&str>)) {
        if get_type().is_some_and(|s| s == "Name") {
            Some("opt_name")
        } else {
            Some("opt_str")
        }
    } else if rt == &(parse_quote!(u32)) {
        Some("required_u32")
    } else if rt == &(parse_quote!(Option<u32>)) {
        Some("opt_u32")
    } else if rt == &(parse_quote!(i32)) {
        Some("required_int")
    } else if rt == &(parse_quote!(Option<i32>)) {
        Some("opt_int")
    } else if rt == &(parse_quote!(f32)) {
        Some("required_f32")
    } else if rt == &(parse_quote!(Option<f32>)) {
        Some("opt_f32")
    } else if rt == &(parse_quote!(Option<u8>)) {
        Some("opt_u8")
    } else if rt == &(parse_quote!(Option<bool>)) {
        Some("opt_bool")
    } else if rt == &(parse_quote!(bool)) {
        Some("required_bool")
    } else if rt == &(parse_quote!(Vec<&Stream<'a>>)) {
        Some("opt_single_or_arr_stream")
    } else if rt == &(parse_quote!(Vec<u32>)) {
        if get_type().is_some_and(|s| s == "Ref") {
            unreachable!()
        } else {
            Some("u32_arr")
        }
    } else if rt == &(parse_quote!(Vec<NonZeroU32>)) {
        if get_type().is_some_and(|s| s == "Ref") {
            Some("ref_id_arr")
        } else {
            unreachable!()
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

fn gen_option_method(
    ty: Either<&Type, &Type>,
    key: &str,
    f_left: impl FnOnce(&Type) -> proc_macro2::TokenStream,
    f_right: impl FnOnce(&Type) -> proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    match ty {
        Left(t) => {
            let body = f_left(unwrap_option_type(t));
            quote! ( #body.context(#key) )
        }
        Right(t) => {
            let body = f_right(t);
            quote! ( #body.context(#key) )
        }
    }
}

fn get_literal_from_some_call(c: &ExprCall) -> &Expr {
    if let Expr::Path(ep) = &*c.func {
        if let Some(seg) = ep.path.segments.last() {
            if seg.ident == "Some" {
                return &c.args[0];
            }
        }
    }
    panic!("expect Some literal")
}

fn get_literal_from_some(t: &Expr) -> &Expr {
    if let Expr::Call(ec) = t {
        return get_literal_from_some_call(ec);
    }
    panic!("expect Some literal")
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
                    parse_quote! {
                        crate::object::ValueTypeValidator<
                            crate::object::NameTypeValueGetter,
                            crate::object::EqualTypeValueChecker<&'static str>
                        >
                    },
                    parse_quote! {
                        crate::object::ValueTypeValidator::new(
                            crate::object::NameTypeValueGetter::typ(),
                            crate::object::EqualTypeValueChecker::new(#lit)
                        )
                    },
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
            let t = get_literal_from_some(t);
            assert!(matches!(
                st,
                Expr::Lit(ExprLit {
                    lit: Lit::Str(_),
                    attrs: _
                })
            ));
            (
                parse_quote! {
                    crate::object::AndValueTypeValidator<
                        crate::object::ValueTypeValidator<
                            crate::object::NameTypeValueGetter,
                            crate::object::OptionTypeValueChecker<crate::object::EqualTypeValueChecker<&'static str>>
                        >,
                        crate::object::ValueTypeValidator<
                            crate::object::NameTypeValueGetter,
                            crate::object::EqualTypeValueChecker<&'static str>
                        >
                    >
                },
                parse_quote! {
                    crate::object::AndValueTypeValidator::new(
                        crate::object::ValueTypeValidator::new(
                            crate::object::NameTypeValueGetter::typ(),
                            <crate::object::EqualTypeValueChecker<&'static str> as crate::object::TypeValueCheck<_>>::option(crate::object::EqualTypeValueChecker::new(#t)),
                        ),
                        crate::object::ValueTypeValidator::new(
                            crate::object::NameTypeValueGetter::new("Subtype"),
                            crate::object::EqualTypeValueChecker::new(#st)
                        ),
                    )
                },
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
            (
                parse_quote! {
                    crate::object::ValueTypeValidator<
                        crate::object::NameTypeValueGetter,
                        crate::object::OneOfTypeValueChecker<&'static str>,
                    >
                },
                parse_quote! {
                    crate::object::ValueTypeValidator::new(
                        crate::object::NameTypeValueGetter::typ(),
                        crate::object::OneOfTypeValueChecker::new(
                            vec![ #(#arg),* ]
                        )
                    )
                },
            )
        }

        Expr::Call(ref call) => {
            let literal = get_literal_from_some_call(call);
            (
                parse_quote! {
                    crate::object::ValueTypeValidator<
                        crate::object::NameTypeValueGetter,
                        crate::object::OptionTypeValueChecker<crate::object::EqualTypeValueChecker<&'static str>>
                    >
                },
                parse_quote! {
                    crate::object::ValueTypeValidator::new(
                        crate::object::NameTypeValueGetter::typ(),
                        <crate::object::EqualTypeValueChecker<&'static str> as crate::object::TypeValueCheck<_>>::option(crate::object::EqualTypeValueChecker::new(#literal)),
                    )
                },
            )
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
        let TraitItem::Fn(TraitItemFn { sig, attrs, .. }) = item  else {
            panic!("only support function")
        };

        let name = sig.ident.clone();

        let default_attr = parse_default_attr(attrs);
        let altered_rt_store: Type;
        let mut rt: &Type = match &sig.output {
            ReturnType::Default => panic!("function must have return type"),
            ReturnType::Type(_, ty) => ty,
        };
        if default_attr.is_some() {
            // if has default attribute, change return type `rt` to `Option<rt>`
            altered_rt_store = parse_quote! { Option<#rt> };
            rt = &altered_rt_store;
        }

        let key = key_attr(attrs).unwrap_or_else(|| snake_case_to_pascal(&name.to_string()));

        let mut method = if let Some(method_name) =
            schema_method_name(rt, &attrs[..]).map(|m| Ident::new(m, name.span()))
        {
            quote! { self.d.#method_name(#key) }
        } else if let Some(nested_type) = nested(rt, attrs) {
            gen_option_method(
                nested_type,
                &key,
                |ty| {
                    let type_name = remove_generic(ty);
                    quote! { self.d.resolver().opt_resolve_container_pdf_object::<_, #type_name>(self.d.dict(), #key) }
                },
                |ty| {
                    if is_vec(ty) {
                        quote! { self.d.resolver().resolve_container_pdf_object_array(self.d.dict(), #key) }
                    } else if is_map(ty) {
                        quote! { self.d.resolver().resolve_container_pdf_object_map(self.d.dict(), #key) }
                    } else {
                        let type_name = remove_generic(ty);
                        quote! { self.d.resolver().resolve_container_pdf_object::<_, #type_name>(self.d.dict(), #key) }
                    }
                },
            )
        } else if let Some(from_name_str_type) = from_name_str(rt, attrs) {
            gen_option_method(
                from_name_str_type,
                &key,
                |ty| {
                    quote! { self.d.opt_name(#key).context(#key)?.map(|s| <#ty as std::str::FromStr>::from_str(s)).transpose() }
                },
                |ty| {
                    quote! { <#ty as std::str::FromStr>::from_str( self.d.required_name(#key).unwrap()) }
                },
            )
        } else if let Some(try_from_type) = try_from(rt, attrs) {
            gen_option_method(
                try_from_type,
                &key,
                |ty| {
                    quote! { self.d.opt_object(#key).context(#key)?.map(|d| <#ty as std::convert::TryFrom<&crate::object::Object>>::try_from(d)).transpose() }
                },
                |ty| {
                    quote! { <#ty as std::convert::TryFrom<&crate::object::Object>>::try_from( self.d.required_object(#key).unwrap()) }
                },
            )
        } else {
            panic!("unsupported return type")
        };

        if let Some(default_attr) = default_attr {
            // unwrap Option<> type from rt
            rt = unwrap_option_type(rt);
            method = match default_attr {
                DefaultAttr::Function(_) => todo!(),
                DefaultAttr::Literal(lit) => quote!( #method.map(|v| v.unwrap_or(#lit))),
                DefaultAttr::OrDefault => quote!( #method.map(|v| v.unwrap_or_default())),
            }
        }

        let method = quote! {
            fn #name(&self) -> anyhow::Result<#rt> {
                use anyhow::Context;
                #method.context(#key)
            }
        };

        methods.push(method);
    }

    let vis = &def.vis;
    let tokens = quote! {
        #[derive(Clone, Debug)]
        #vis struct #struct_name<'a, 'b> {
            d: SchemaDict<'a, 'b, #valid_ty>,
            id: Option<std::num::NonZeroU32>,
        }

        impl<'a, 'b> crate::object::PdfObject<'a, 'b> for #struct_name<'a, 'b> {
            fn new(id: Option<std::num::NonZeroU32>, dict: &'b Dictionary<'a>, r: &'b ObjectResolver<'a>) -> Result<Self, ObjectValueError> {
                let d = SchemaDict::new(dict, r, #valid_arg)?;
                Ok(Self { d, id})
            }

            fn checked(id: Option<std::num::NonZeroU32>, dict: &'b Dictionary<'a>, r: &'b ObjectResolver<'a>) -> Result<Option<Self>, ObjectValueError> {
                let d = SchemaDict::from(dict, r, #valid_arg)?;
                Ok(d.map(|d| Self { d, id}))
            }

            fn id(&self) -> Option<std::num::NonZeroU32> {
                self.id
            }
        }

        impl<'a, 'b> #struct_name<'a, 'b> {
            #(pub #methods)*
        }
    };

    // println!("{}", tokens);
    tokens.into()
}
