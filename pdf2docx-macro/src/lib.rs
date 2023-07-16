use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{
    parse_macro_input, parse_quote, Arm, Expr, ExprLit, Ident, ItemEnum, Lit, LitStr, Pat, Token,
};

#[proc_macro_attribute]
pub fn graphics_operation_parser(_args: TokenStream, mut input: TokenStream) -> TokenStream {
    let op_enum = input.clone();
    let _op_enum = parse_macro_input!(op_enum as ItemEnum);
    let mut arms = vec![];
    let operation_value = |s| {
        let op = Ident::new("Operation", Span::call_site());
        let s = Ident::new(s, Span::call_site());
        let r: Expr = parse_quote!( #op::#s );
        r
    };
    let pattern = |s| {
        Pat::Lit(ExprLit {
            attrs: vec![],
            lit: Lit::Str(LitStr::new(s, Span::call_site())),
        })
    };
    let ident = |s| Ident::new(s, Span::call_site());
    let convert_from_object = || ident("convert_from_object");
    let arm = |s, body: Expr| Arm {
        pat: pattern(s),
        guard: None,
        body: body.into(),
        comma: None,
        attrs: vec![],
        fat_arrow_token: Token![=>](Span::call_site()),
    };

    // "q" -> Operation::SaveGraphicsState
    arms.push(arm("q", operation_value("SaveGraphicsState")));
    // "Q" -> Operation::RestoreGraphicsState
    arms.push(arm("Q", operation_value("RestoreGraphicsState")));
    // "w" => Operation::SetLineWidth(f32::convert_from_object(operands)?)
    arms.push(arm("w", {
        let op = operation_value("SetLineWidth");
        let convert_from_object = convert_from_object();
        parse_quote!( #op(f32::#convert_from_object(operands)?) )
    }));

    let tokens = quote! {
        fn create_operation(op: &str, operands: &mut Vec<Object>) -> Result<Operation, ObjectValueError> {
            Ok(match op {
                #( #arms, )*
                // "q" => Operation::SaveGraphicsState,
                // "Q" => Operation::RestoreGraphicsState,
                // "w" => Operation::SetLineWidth(f32::convert_from_object(operands)?),
                _ => todo!(),
            })
        }
    };
    let tokens: TokenStream = tokens.into();
    input.extend(tokens);
    input
}
