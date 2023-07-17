use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{
    parse_macro_input, parse_quote, Arm, Expr, ExprLit, ExprParen, Ident, ItemEnum, Lit, LitStr,
    Meta, Pat, Token, Variant,
};

#[proc_macro_derive(OperationParser, attributes(op_tag))]
pub fn graphics_operation_parser(mut input: TokenStream) -> TokenStream {
    let op_enum = input.clone();
    let op_enum = parse_macro_input!(op_enum as ItemEnum);
    fn operation_value(s: &str) -> Expr {
        let op = Ident::new("Operation", Span::call_site());
        let s = Ident::new(s, Span::call_site());
        parse_quote!( #op::#s )
    }
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
        arms.push(arm(&s.expect("op_tag not defined"), op));
    }
    arms.push(arm("q", operation_value("SaveGraphicsState")));
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
                _ => todo!("haha"),
            })
        }
    };
    eprintln!("{}", tokens);
    tokens.into()
}
