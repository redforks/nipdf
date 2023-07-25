use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{
    parse_macro_input, parse_quote, Arm, Expr, ExprLit, Fields, FieldsUnnamed, Ident, ItemEnum,
    Lit, LitStr, Meta, Pat, Token,
};

/// Generate `impl TryFrom` for enum that convert Object::Name to enum variant
/// Name equals to variant name
#[proc_macro_derive(TryFromNameObject)]
pub fn try_from_name_object(input: TokenStream) -> TokenStream {
    let enum_t = parse_macro_input!(input as ItemEnum);
    let t = enum_t.ident;
    let arms = enum_t
        .variants
        .iter()
        .map(|branch| -> proc_macro2::TokenStream {
            let b = &branch.ident;
            let lit = b.to_string();
            parse_quote!( #lit => Ok(#t::#b))
        });
    let tokens = quote! {
        impl<'a, 'b> TryFrom<&'b Object<'a>> for #t {
            type Error = ObjectValueError;
            fn try_from(object: &'b Object<'a>) -> Result<Self, Self::Error> {
                match object.as_name()? {
                    #( #arms, )*
                    _ => Err(ObjectValueError::GraphicsOperationSchemaError),
                }
            }
        }
    };
    // println!("{}", tokens);
    tokens.into()
}

/// impl TryFrom trait for enum that convert Object::Int to enum variant
#[proc_macro_derive(TryFromIntObject)]
pub fn try_from_int_object(input: TokenStream) -> TokenStream {
    let enum_t = parse_macro_input!(input as ItemEnum);
    let t = enum_t.ident;
    let arms = enum_t.variants.iter().map(|branch| -> proc_macro2::TokenStream {
        let Some((_, Expr::Lit(ExprLit{lit: Lit::Int(ref lit), ..}))) = branch.discriminant else {
            panic!("Enum discriminant must be literal");
        };
        let digit: i32 = lit.base10_parse().unwrap();
        let b = &branch.ident;
        parse_quote!( #digit=> Ok(#t::#b))
    });
    let tokens = quote! {
        impl<'a, 'b> TryFrom<&'b Object<'a>> for #t {
            type Error = ObjectValueError;
            fn try_from(object: &'b Object<'a>) -> Result<Self, Self::Error> {
                let n = object.as_int()?;
                match n {
                    #( #arms, )*
                    _ => Err(ObjectValueError::GraphicsOperationSchemaError),
                }
            }
        }
    };
    // println!("{}", tokens);
    tokens.into()
}

#[proc_macro_derive(OperationParser, attributes(op_tag))]
pub fn graphics_operation_parser(input: TokenStream) -> TokenStream {
    let op_enum = parse_macro_input!(input as ItemEnum);
    let new_arm = |s: &str, body: Expr| Arm {
        pat: Pat::Lit(ExprLit {
            attrs: vec![],
            lit: Lit::Str(LitStr::new(s, Span::call_site())),
        }),
        guard: None,
        body: body.into(),
        comma: None,
        attrs: vec![],
        fat_arrow_token: Token![=>](Span::call_site()),
    };

    let mut arms = vec![];
    for branch in op_enum.variants {
        let mut convert_args: Vec<Expr> = vec![];
        if !branch.fields.is_empty() {
            if let Fields::Unnamed(FieldsUnnamed {
                unnamed: fields, ..
            }) = branch.fields
            {
                for f in fields {
                    let t = f.ty;
                    convert_args.push(
                        parse_quote!( <#t as ConvertFromObject>::convert_from_object(operands)?),
                    );
                }
            }
        }
        let op = branch.ident;
        let op: Expr = parse_quote!(Operation::#op);
        let mut s = None;
        for attr in &branch.attrs {
            if let Meta::List(ref list) = attr.meta {
                if list.path.is_ident("op_tag") {
                    let tokens: TokenStream = list.tokens.clone().into();
                    if let ExprLit {
                        lit: Lit::Str(lit), ..
                    } = parse_macro_input!(tokens as ExprLit)
                    {
                        s = Some(lit.value());
                        break;
                    }
                }
            }
        }

        arms.push(new_arm(
            &s.expect("op_tag not defined"),
            match convert_args.len() {
                0 => op,
                1 => parse_quote!( #op(#(#convert_args),*) ),
                _ => {
                    let mut save_to_vars = vec![];
                    let mut vars = vec![];
                    for (idx, arg) in convert_args.into_iter().enumerate() {
                        // store arg result in variable _arg_idx
                        let var = Ident::new(&(format!("_arg_{}", idx)), Span::call_site());
                        vars.push(var.clone());
                        save_to_vars.push(quote!( let #var = #arg; ));
                    }
                    save_to_vars.reverse();
                    parse_quote!( {
                        #( #save_to_vars )*
                        #op(#(#vars),*)
                    })
                }
            },
        ));
    }
    // "w" => Operation::SetLineWidth(f32::convert_from_object(operands)?)
    // arms.push(arm("w", {
    //     let op = operation_value("SetLineWidth");
    //     let convert_from_object = convert_from_object();
    //     parse_quote!( #op(f32::#convert_from_object(operands)?) )
    // }));

    let tokens = quote! {
        fn create_operation<'a>(op: &str, operands: &mut Vec<Object<'a>>) -> Result<Operation<'a>, ObjectValueError> {
            Ok(match op {
                #( #arms, )*
                _ => todo!(),
            })
        }
    };
    // println!("{}", tokens);
    tokens.into()
}

mod pdf_object_impl;

#[proc_macro_attribute]
pub fn pdf_object(attr: TokenStream, item: TokenStream) -> TokenStream {
    pdf_object_impl::pdf_object(attr, item)
}
