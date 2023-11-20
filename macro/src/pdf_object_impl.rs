use core::panic;
use either::{Either, Left, Right};
use proc_macro::TokenStream;
use proc_macro2::Ident;
use quote::{quote, ToTokens};
use syn::{
    parse_macro_input, parse_quote, Attribute, Expr, ExprCall, ExprLit, ExprPath, ExprTuple,
    ItemTrait, Lit, LitStr, Meta, ReturnType, TraitItem, TraitItemFn, Type,
};

/// If `#[key("key")]` attribute defined, return key value
fn key_attr(attrs: &[Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| {
        attr.path().is_ident("key").then(|| {
            let lit: LitStr = attr.parse_args().expect("expect string literal");
            lit.value()
        })
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

    let Some(seg) = tp.path.segments.last() else {
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
        attr.path().is_ident("default").then(|| {
            let lit: ExprLit = attr.parse_args().expect("expect literal");
            lit
        })
    })
}

/// Return Some(func_name) if `#[default_fn(func)]` attribute defined, otherwise return None
fn default_fn(attrs: &[Attribute]) -> Option<ExprPath> {
    attrs.iter().find_map(|attr| {
        attr.path().is_ident("default_fn").then(|| {
            let lit: ExprPath = attr.parse_args().expect("expect function");
            lit
        })
    })
}

/// Return true if `#[or_default]` attribute defined.
fn or_default(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| attr.path().is_ident("or_default"))
}

/// Return true if `#[stub_resolver]` attribute defined.
fn stub_resolver(attrs: &[Attribute]) -> bool {
    attrs
        .iter()
        .any(|attr| attr.path().is_ident("stub_resolver"))
}

enum DefaultAttr {
    Literal(ExprLit),
    Function(ExprPath),
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

/// Return true if `#[one_or_more]` attribute defined.
fn one_or_more(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| attr.path().is_ident("one_or_more"))
}

fn self_as<'a>(rt: &'a Type, attrs: &'a [Attribute]) -> Option<Either<&'a Type, &'a Type>> {
    has_attr("self_as", rt, attrs)
}

/// Return left means Option<T>, right means T, Return None means `try_from` attr not defined.
fn try_from<'a>(rt: &'a Type, attrs: &'a [Attribute]) -> Option<Either<&'a Type, &'a Type>> {
    has_attr("try_from", rt, attrs)
}

fn schema_method_name(rt: &Type, attrs: &[Attribute]) -> Option<&'static str> {
    let get_type = || {
        attrs.iter().find_map(|attr| {
            attr.path().is_ident("typ").then(|| {
                let lit: LitStr = attr.parse_args().expect("expect string literal");
                lit.value()
            })
        })
    };

    if rt == &(parse_quote! { &Name }) || rt == &(parse_quote! { &'b Name }) {
        Some("required_name")
    } else if rt == &(parse_quote! { &str }) || rt == &(parse_quote!(&'b str)) {
        Some("required_str")
    } else if rt == &(parse_quote!(Option<&Name>)) || rt == &(parse_quote!(Option<&'b Name>)) {
        Some("opt_name")
    } else if rt == &(parse_quote!(Option<&str>)) || rt == &(parse_quote!(Option<&'b str>)) {
        Some("opt_str")
    } else if rt == &(parse_quote!(u32)) {
        Some("required_u32")
    } else if rt == &(parse_quote!(Option<u32>)) {
        Some("opt_u32")
    } else if rt == &(parse_quote!(u16)) {
        Some("required_u16")
    } else if rt == &(parse_quote!(Option<u16>)) {
        Some("opt_u16")
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
    } else if rt == &(parse_quote!(Vec<f32>)) {
        Some("f32_arr")
    } else if rt == &(parse_quote!(Option<Vec<f32>>)) {
        Some("opt_f32_arr")
    } else if rt == &(parse_quote!(Option<&'b Dictionary<'a>>)) {
        Some("opt_dict")
    } else if rt == &(parse_quote!(&'b Dictionary<'a>)) {
        Some("required_dict")
    } else if rt == &(parse_quote!(Option<&'b Stream<'a>>)) {
        Some("opt_stream")
    } else if rt == &(parse_quote!(NonZeroU32)) {
        Some("required_ref")
    } else if rt == &(parse_quote!(Option<NonZeroU32>)) {
        Some("opt_ref")
    } else if rt == &(parse_quote!(&[u8])) {
        Some("as_byte_string")
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

/// `t` should be literal or `Some(literal)`, return `Left` if `t` is literal, return `Right` if `t`
/// is `Some(literal)
fn get_literal_from_possible_some(t: &Expr) -> Either<&Expr, &Expr> {
    if let Expr::Call(ec) = t {
        Either::Right(get_literal_from_some_call(ec))
    } else {
        // assert `t` is str literal
        assert!(matches!(
            t,
            Expr::Lit(ExprLit {
                lit: Lit::Str(_),
                attrs: _
            })
        ));
        Either::Left(t)
    }
}

fn type_field(attrs: &[Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| {
        attr.path().is_ident("type_field").then(|| {
            let lit: LitStr = attr.parse_args().expect("expect string literal");
            lit.value()
        })
    })
}

fn doc(attrs: &[Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| {
        if let Meta::NameValue(name_value) = &attr.meta {
            if name_value.path.is_ident("doc") {
                if let Expr::Lit(ExprLit {
                    lit: Lit::Str(lit), ..
                }) = &name_value.value
                {
                    return Some(lit.value());
                }
            }
        }
        None
    })
}

pub fn pdf_object(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr_expr = parse_macro_input!(attr as Expr);
    let def = parse_macro_input!(item as ItemTrait);

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
                syn::Lit::Str(lit) => {
                    let typ_field =
                        type_field(def.attrs.as_slice()).unwrap_or_else(|| "Type".to_owned());
                    (
                        parse_quote! {
                            crate::object::ValueTypeValidator<
                                crate::object::NameTypeValueGetter,
                                crate::object::EqualTypeValueChecker<prescript::Name>
                            >
                        },
                        parse_quote! {
                            crate::object::ValueTypeValidator::new(
                                crate::object::NameTypeValueGetter::new(prescript_macro::name!(#typ_field)),
                                crate::object::EqualTypeValueChecker::new(prescript_macro::name!(#lit))
                            )
                        },
                    )
                }
                syn::Lit::Int(lit) => {
                    let typ_field =
                        type_field(def.attrs.as_slice()).unwrap_or_else(|| "Type".to_owned());
                    (
                        parse_quote! {
                            crate::object::ValueTypeValidator<
                                crate::object::IntTypeValueGetter,
                                crate::object::EqualTypeValueChecker<i32>
                            >
                        },
                        parse_quote! {
                            crate::object::ValueTypeValidator::new(
                                crate::object::IntTypeValueGetter::new(prescript_macro::name!(#typ_field)),
                                crate::object::EqualTypeValueChecker::new(#lit)
                            )
                        },
                    )
                }
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
            let t = get_literal_from_possible_some(t);
            assert!(matches!(
                st,
                Expr::Lit(ExprLit {
                    lit: Lit::Str(_),
                    attrs: _
                })
            ));
            let typ_field = type_field(def.attrs.as_slice()).unwrap_or_else(|| "Type".to_owned());
            let checker = t.map_either(
                    |t| -> Expr {parse_quote!{<crate::object::EqualTypeValueChecker<prescript::Name> as crate::object::TypeValueCheck<_>>::option(crate::object::EqualTypeValueChecker::new(prescript_macro::name!(#t)))}},
                    |t| -> Expr {parse_quote!{crate::object::EqualTypeValueChecker::new(prescript_macro::name!(#t))} },
                ).into_inner();
            let checker_type = t.map_either(
                    |_| -> Type {parse_quote!{crate::object::OptionTypeValueChecker<crate::object::EqualTypeValueChecker<prescript::Name>>}},
                    |_| -> Type {parse_quote!{crate::object::EqualTypeValueChecker<prescript::Name>}},
                ).into_inner();
            (
                parse_quote! {
                    crate::object::AndValueTypeValidator<
                        crate::object::ValueTypeValidator<
                            crate::object::NameTypeValueGetter,
                            #checker_type,
                        >,
                        crate::object::ValueTypeValidator<
                            crate::object::NameTypeValueGetter,
                            crate::object::EqualTypeValueChecker<prescript::Name>
                        >
                    >
                },
                parse_quote! {
                    crate::object::AndValueTypeValidator::new(
                        crate::object::ValueTypeValidator::new(
                            crate::object::NameTypeValueGetter::new(prescript_macro::name!(#typ_field)),
                            #checker,
                        ),
                        crate::object::ValueTypeValidator::new(
                            crate::object::NameTypeValueGetter::new(prescript_macro::name!("Subtype")),
                            crate::object::EqualTypeValueChecker::new(prescript_macro::name!(#st))
                        ),
                    )
                },
            )
        }

        Expr::Array(arr) => {
            let mut arg = vec![];
            for expr in arr.elems {
                match expr {
                    Expr::Lit(lit) => {
                        let lit = lit.lit;
                        match lit {
                            syn::Lit::Str(lit) => {
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
            let typ_field = type_field(def.attrs.as_slice()).unwrap_or_else(|| "Type".to_owned());
            (
                parse_quote! {
                    crate::object::ValueTypeValidator<
                        crate::object::NameTypeValueGetter,
                        crate::object::OneOfTypeValueChecker<prescript::Name>,
                    >
                },
                parse_quote! {
                    crate::object::ValueTypeValidator::new(
                        crate::object::NameTypeValueGetter::new(prescript_macro::name!(#typ_field)),
                        crate::object::OneOfTypeValueChecker::new(
                            vec![ #(prescript_macro::name!(#arg)),* ]
                        )
                    )
                },
            )
        }

        Expr::Call(ref call) => {
            let literal = get_literal_from_some_call(call);
            let typ_field = type_field(def.attrs.as_slice()).unwrap_or_else(|| "Type".to_owned());
            (
                parse_quote! {
                    crate::object::ValueTypeValidator<
                        crate::object::NameTypeValueGetter,
                        crate::object::OptionTypeValueChecker<crate::object::EqualTypeValueChecker<prescript::Name>>
                    >
                },
                parse_quote! {
                    crate::object::ValueTypeValidator::new(
                        crate::object::NameTypeValueGetter::new(prescript_macro::name!(#typ_field)),
                        <crate::object::EqualTypeValueChecker<prescript::Name> as crate::object::TypeValueCheck<_>>::option(crate::object::EqualTypeValueChecker::new(prescript_macro::name!(#literal))),
                    )
                },
            )
        }
        _ => todo!(),
    };

    let name = def.ident.to_string();
    assert!(name.ends_with("Trait"));
    let struct_name = &name[..name.len() - 5];
    let struct_name = Ident::new(struct_name, def.ident.span());

    let mut methods = vec![];
    for item in &def.items {
        let TraitItem::Fn(TraitItemFn { sig, attrs, .. }) = item else {
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
            quote! { self.d.#method_name(prescript_macro::name!(#key)) }
        } else if let Some(nested_type) = nested(rt, attrs) {
            gen_option_method(
                nested_type,
                &key,
                |ty| {
                    let type_name = remove_generic(ty);
                    quote! { self.d.opt_resolve_pdf_object::<#type_name>(prescript_macro::name!(#key)) }
                },
                |ty| {
                    if is_vec(ty) {
                        if one_or_more(attrs) {
                            quote! { self.d.resolve_one_or_more_pdf_object(prescript_macro::name!(#key)) }
                        } else {
                            quote! { self.d.resolve_pdf_object_array(prescript_macro::name!(#key)) }
                        }
                    } else if is_map(ty) {
                        quote! { self.d.resolve_pdf_object_map(prescript_macro::name!(#key)) }
                    } else {
                        let type_name = remove_generic(ty);
                        quote! { self.d.resolve_pdf_object::<#type_name>(prescript_macro::name!(#key)) }
                    }
                },
            )
        } else if let Some(try_from_type) = try_from(rt, attrs) {
            gen_option_method(
                try_from_type,
                &key,
                |ty| {
                    quote! { self.d.opt_object(prescript_macro::name!(#key)).context(#key)?.map(|d| <#ty as std::convert::TryFrom<&crate::object::Object>>::try_from(d)).transpose() }
                },
                |ty| {
                    quote! { <#ty as std::convert::TryFrom<&crate::object::Object>>::try_from( self.d.required_object(prescript_macro::name!(#key)).unwrap()) }
                },
            )
        } else if let Some(rt) = self_as(rt, attrs) {
            gen_option_method(
                rt,
                &key,
                |_| unreachable!("self_as methods never return Option"),
                |ty| {
                    quote! { <#ty as crate::object::PdfObject::<_>>::new(self.id, self.d.dict(), self.d.resolver()) }
                },
            )
        } else {
            panic!("unsupported return type: {}", rt.to_token_stream())
        };

        if let Some(default_attr) = default_attr {
            // unwrap Option<> type from rt
            rt = unwrap_option_type(rt);
            method = match default_attr {
                DefaultAttr::Function(func) => quote!( #method.map(|v| v.unwrap_or_else(#func))),
                DefaultAttr::Literal(lit) => quote!( #method.map(|v| v.unwrap_or(#lit))),
                DefaultAttr::OrDefault => quote!( #method.map(|v| v.unwrap_or_default())),
            }
        }

        let method = if let Some(doc) = doc(attrs) {
            quote! {
                #[doc = #doc]
                pub fn #name(&self) -> anyhow::Result<#rt> {
                    use anyhow::Context;
                    #method.context(#key)
                }
            }
        } else {
            quote! {
                pub fn #name(&self) -> anyhow::Result<#rt> {
                    use anyhow::Context;
                    #method.context(#key)
                }
            }
        };
        methods.push(method);
    }

    let vis = &def.vis;
    let tokens = if stub_resolver(&def.attrs) {
        quote! {
            #[derive(Clone, Debug)]
            #vis struct #struct_name<'a, 'b> {
                d: crate::object::SchemaDict<'a, 'b, #valid_ty, ()>,
                id: Option<std::num::NonZeroU32>,
            }

            impl<'a, 'b> crate::object::PdfObject<'a, 'b, ()> for #struct_name<'a, 'b> {
                fn new(id: Option<std::num::NonZeroU32>, dict: &'b crate::object::Dictionary<'a>, r: &'b ()) -> Result<Self, crate::object::ObjectValueError> {
                    let d = crate::object::SchemaDict::new(dict, r, #valid_arg)?;
                    Ok(Self { d, id})
                }

                fn checked(id: Option<std::num::NonZeroU32>, dict: &'b crate::object::Dictionary<'a>, r: &'b ()) -> Result<Option<Self>, crate::object::ObjectValueError> {
                    let d = crate::object::SchemaDict::from(dict, r, #valid_arg)?;
                    Ok(d.map(|d| Self { d, id}))
                }

                fn id(&self) -> Option<std::num::NonZeroU32> {
                    self.id
                }

                fn resolver(&self) -> &'b () {
                    self.d.resolver()
                }
            }

            impl<'a, 'b> #struct_name<'a, 'b> {
                #(#methods)*
            }
        }
    } else {
        quote! {
            #[derive(Clone, Debug)]
            #vis struct #struct_name<'a, 'b> {
                d: crate::object::SchemaDict<'a, 'b, #valid_ty, crate::file::ObjectResolver<'a>>,
                id: Option<std::num::NonZeroU32>,
            }

            impl<'a, 'b> crate::object::PdfObject<'a, 'b, crate::file::ObjectResolver<'a>> for #struct_name<'a, 'b> {
                fn new(id: Option<std::num::NonZeroU32>, dict: &'b crate::object::Dictionary<'a>, r: &'b crate::file::ObjectResolver<'a>) -> Result<Self, crate::object::ObjectValueError> {
                    let d = crate::object::SchemaDict::new(dict, r, #valid_arg)?;
                    Ok(Self { d, id})
                }

                fn checked(id: Option<std::num::NonZeroU32>, dict: &'b crate::object::Dictionary<'a>, r: &'b crate::file::ObjectResolver<'a>) -> Result<Option<Self>, crate::object::ObjectValueError> {
                    let d = crate::object::SchemaDict::from(dict, r, #valid_arg)?;
                    Ok(d.map(|d| Self { d, id}))
                }

                fn id(&self) -> Option<std::num::NonZeroU32> {
                    self.id
                }

                fn resolver(&self) -> &'b crate::file::ObjectResolver<'a> {
                    self.d.resolver()
                }
            }

            impl<'a, 'b> #struct_name<'a, 'b> {
                #(#methods)*
            }
        }
    };

    // println!("{}", tokens);
    tokens.into()
}
