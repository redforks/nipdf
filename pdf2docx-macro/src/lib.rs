use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{
    parse_macro_input, parse_quote, Arm, Expr, ExprLit, Fields, FieldsUnnamed, Ident, ItemEnum,
    Lit, LitStr, Meta, Pat, Token,
};

#[proc_macro_derive(OperationParser, attributes(op_tag))]
pub fn graphics_operation_parser(input: TokenStream) -> TokenStream {
    let op_enum = input;
    let op_enum = parse_macro_input!(op_enum as ItemEnum);
    let operation_value_from_ident = |i: Ident| {
        let op = Ident::new("Operation", Span::call_site());
        let r: Expr = parse_quote!( #op::#i );
        r
    };
    let ident = |s| Ident::new(s, Span::call_site());
    let convert_from_object = || ident("convert_from_object");
    let arm = |s: &str, body: Expr| Arm {
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
                    let f = f.ty.clone();
                    let convert_from_object = convert_from_object();
                    convert_args.push(parse_quote!( #f::#convert_from_object(operands)?));
                }
            }
        }
        let op = operation_value_from_ident(branch.ident.clone());
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
        if convert_args.is_empty() {
            arms.push(arm(&s.expect("op_tag not defined"), op));
        } else {
            arms.push(arm(
                &s.expect("op_tag not defined"),
                parse_quote!( #op(#(#convert_args),*) ),
            ));
        }
    }
    // "w" => Operation::SetLineWidth(f32::convert_from_object(operands)?)
    // arms.push(arm("w", {
    //     let op = operation_value("SetLineWidth");
    //     let convert_from_object = convert_from_object();
    //     parse_quote!( #op(f32::#convert_from_object(operands)?) )
    // }));

    let tokens = quote! {
        fn create_operation(op: &str, operands: &mut Vec<Object>) -> Result<Operation, ObjectValueError> {
            Ok(match op {
                #( #arms, )*
                _ => todo!(),
            })
        }
    };
    tokens.into()
}
