use core::panic;
use either::Either;
use proc_macro::TokenStream;
use proc_macro2::Ident;
use quote::{ToTokens, quote};
use syn::{
    Attribute, Expr, ExprCall, ExprLit, ExprPath, ExprTuple, ItemTrait, Lit, LitStr, Meta,
    ReturnType, TraitItem, TraitItemFn, Type, parse_macro_input, parse_quote,
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
        return Some(Either::Right(rt));
    };

    let Some(seg) = tp.path.segments.last() else {
        panic!("expect path segment")
    };

    if seg.ident != "Option" {
        return Some(Either::Right(rt));
    }

    Some(Either::Left(rt))
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

    if rt == &(parse_quote! { Name })
        || rt == &(parse_quote!(&'b str))
        || rt == &(parse_quote!(u32))
        || rt == &(parse_quote!(u16))
        || rt == &(parse_quote!(i32))
        || rt == &(parse_quote!(f32))
        || rt == &(parse_quote!(bool))
        || rt == &(parse_quote!(&'b Dictionary))
        || rt == &(parse_quote!(RuntimeObjectId))
        || rt == &(parse_quote!(&'b [u8]))
    {
        Some("required")
    } else if rt == &(parse_quote!(Option<Name>))
        || rt == &(parse_quote!(Option<&'b str>))
        || rt == &(parse_quote!(Option<u32>))
        || rt == &(parse_quote!(Option<u16>))
        || rt == &(parse_quote!(Option<i32>))
        || rt == &(parse_quote!(Option<f32>))
        || rt == &(parse_quote!(Option<u8>))
        || rt == &(parse_quote!(Option<bool>))
        || rt == &(parse_quote!(Option<Vec<f32>>))
        || rt == &(parse_quote!(Option<&'b Dictionary>))
        || rt == &(parse_quote!(Option<&'b Stream>))
        || rt == &(parse_quote!(Option<RuntimeObjectId>))
        || rt == &(parse_quote!(Option<(f32, f32)>))
    {
        Some("opt")
    } else if rt == &(parse_quote!(Vec<f32>))
        || (rt == &(parse_quote!(Vec<u32>)) && get_type().is_none_or(|s| s != "Ref"))
        || (rt == &(parse_quote!(Vec<RuntimeObjectId>)) && get_type().is_some_and(|s| s == "Ref"))
    {
        Some("or_default")
    } else if rt == &(parse_quote!(Vec<&'b Stream>)) {
        Some("zero_one_or_more")
    } else if rt == &(parse_quote!(HashMap<Name, &'b Stream>)) {
        Some("map_dict")
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
    f_left: impl FnOnce(&Type) -> proc_macro2::TokenStream,
    f_right: impl FnOnce(&Type) -> proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    match ty {
        Either::Left(t) => {
            let body = f_left(unwrap_option_type(t));
            quote! ( #body )
        }
        Either::Right(t) => {
            let body = f_right(t);
            quote! ( #body )
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

/// Return true if `#[root_pdf_object]` attribute defined.
fn is_root_pdf_object(attrs: &[Attribute]) -> bool {
    attrs
        .iter()
        .any(|attr| attr.path().is_ident("root_pdf_object"))
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
                Lit::Str(lit) => {
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
                                crate::object::NameTypeValueGetter::new(prescript::sname(#typ_field)),
                                crate::object::EqualTypeValueChecker::new(prescript::sname(#lit))
                            )
                        },
                    )
                }
                Lit::Int(lit) => {
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
                                crate::object::IntTypeValueGetter::new(prescript::sname(#typ_field)),
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
                    |t| -> Expr {parse_quote!{<crate::object::EqualTypeValueChecker<prescript::Name> as crate::object::TypeValueCheck<_>>::option(crate::object::EqualTypeValueChecker::new(prescript::sname(#t)))}},
                    |t| -> Expr {parse_quote!{crate::object::EqualTypeValueChecker::new(prescript::sname(#t))} },
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
                            crate::object::NameTypeValueGetter::new(prescript::sname(#typ_field)),
                            #checker,
                        ),
                        crate::object::ValueTypeValidator::new(
                            crate::object::NameTypeValueGetter::new(prescript::sname("Subtype")),
                            crate::object::EqualTypeValueChecker::new(prescript::sname(#st))
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
                            Lit::Str(lit) => {
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
                        crate::object::NameTypeValueGetter::new(prescript::sname(#typ_field)),
                        crate::object::OneOfTypeValueChecker::new(
                            vec![ #(prescript::sname(#arg)),* ]
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
                        crate::object::NameTypeValueGetter::new(prescript::sname(#typ_field)),
                        <crate::object::EqualTypeValueChecker<prescript::Name> as crate::object::TypeValueCheck<_>>::option(crate::object::EqualTypeValueChecker::new(prescript::sname(#literal))),
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
            quote! { self.d.#method_name(#key) }
        } else if let Some(nested_type) = nested(rt, attrs) {
            gen_option_method(
                nested_type,
                |ty| {
                    let type_name = remove_generic(ty);
                    quote! { self.d.opt::<#type_name<'_, '_>, _>(#key) }
                },
                |ty| {
                    if is_vec(ty) {
                        quote! { self.d.zero_one_or_more(#key) }
                    } else if is_map(ty) {
                        quote! { self.d.map_dict(#key) }
                    } else {
                        let type_name = remove_generic(ty);
                        quote! { self.d.required::<#type_name<'_, '_>, _>(#key) }
                    }
                },
            )
        } else if let Some(try_from_type) = try_from(rt, attrs) {
            gen_option_method(
                try_from_type,
                |ty| {
                    quote! {
                        let d: Option<crate::object::ObjectWithResolver> = self.d.opt(#key)?;
                        match d.map(|d| <#ty>::try_from(d)).transpose() {
                            Ok(v) => Ok(v),
                            Err(e) => {
                                log::warn!("Convert PDF object field {} to {:?}: {}, ignore its value", #key, stringify!(#ty), e);
                                Ok(None)
                            }
                        }
                    }
                },
                |ty| {
                    quote! {
                        let d: crate::object::ObjectWithResolver = self.d.required(#key)?;
                        <#ty>::try_from(d)
                    }
                },
            )
        } else if let Some(rt) = self_as(rt, attrs) {
            gen_option_method(
                rt,
                |_| unreachable!("self_as methods never return Option"),
                |ty| {
                    quote! { <Self as crate::object::ToPdfObject::<(#ty, _, _)>>::to_pdf_object(self).map(|v| v.0) }
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

        let doc = if let Some(doc) = doc(attrs) {
            quote! { #[doc = #doc] }
        } else {
            quote! {}
        };

        let method = quote! {
            #doc
            pub fn #name(&self) -> std::result::Result<#rt, crate::ObjectValueError> {
                use snafu::ResultExt as _;
                #method
            }
        };
        methods.push(method);
    }

    let vis = &def.vis;
    let tokens = if is_root_pdf_object(def.attrs.as_slice()) {
        quote! {
            #[derive(Clone, Debug)]
            #vis struct #struct_name<'a, 'b> {
                id: crate::object::RuntimeObjectId,
                d: crate::object::SchemaDict<'a, 'b, #valid_ty>,
            }

            impl<'a, 'b> crate::object::RootPdfObject<'a, 'b> for #struct_name<'a, 'b> {
                fn new(id: crate::object::RuntimeObjectId, dict: &'b crate::object::Dictionary, r: &'b crate::file::ObjectResolver<'a>) -> Result<Self, crate::ObjectValueError> {
                    let d = crate::object::SchemaDict::new(dict, r, #valid_arg)?;
                    Ok(Self { id, d })
                }

                fn id(&self) -> crate::object::RuntimeObjectId {
                    self.id
                }
            }

            impl<'a, 'b> crate::object::PdfObjectCore<'a, 'b> for #struct_name<'a, 'b> {
                fn dict(&self) -> &'b crate::object::Dictionary {
                    self.d.dict()
                }

                fn resolver(&self) -> &'b crate::file::ObjectResolver<'a> {
                    self.d.resolver()
                }
            }

            impl<'a, 'b> crate::object::FromSchemaContainer<'a, 'b> for #struct_name<'a, 'b> {
                fn create(o: &'b crate::object::Object, r: &'b crate::file::ObjectResolver<'a>) -> Result<Self, crate::ObjectValueError> {
                    use snafu::{OptionExt as _, ResultExt as _};
                    let id = o
                        .reference()
                        .map(Into::into)
                        .whatever_context::<_, crate::ObjectValueError>("root pdf object need id")?;
                    let o = r.resolve(id)?;
                    <Self as crate::object::RootPdfObject>::new(id, o.as_dict()?, r)
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
                d: crate::object::SchemaDict<'a, 'b, #valid_ty>,
            }

            impl<'a, 'b> crate::object::PdfObject<'a, 'b> for #struct_name<'a, 'b> {
                fn new(dict: &'b crate::object::Dictionary, r: &'b crate::file::ObjectResolver<'a>) -> Result<Self, crate::ObjectValueError> {
                    let d = crate::object::SchemaDict::new(dict, r, #valid_arg)?;
                    Ok(Self { d })
                }
            }

            impl<'a, 'b> crate::object::PdfObjectCore<'a, 'b> for #struct_name<'a, 'b> {
                fn dict(&self) -> &'b crate::object::Dictionary {
                    self.d.dict()
                }

                fn resolver(&self) -> &'b crate::file::ObjectResolver<'a> {
                    self.d.resolver()
                }
            }

            impl<'a, 'b> crate::object::FromSchemaContainer<'a, 'b> for #struct_name<'a, 'b> {
                fn create(o: &'b crate::object::Object, r: &'b crate::file::ObjectResolver<'a>) -> Result<Self, crate::ObjectValueError> {
                    let o = r.resolve_reference(o)?;
                    <Self as crate::object::PdfObject>::new(o.as_dict()?, r)
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
