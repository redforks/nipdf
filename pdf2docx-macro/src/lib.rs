use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{
    parse_macro_input, punctuated::Punctuated, Arm, Expr, ExprCall, ExprLit, ExprPath, ExprTry,
    Ident, ItemEnum, Lit, LitStr, Pat, Path, PathArguments, PathSegment, Token,
};

#[proc_macro_attribute]
pub fn graphics_operation_parser(_args: TokenStream, mut input: TokenStream) -> TokenStream {
    let op_enum = input.clone();
    let op_enum = parse_macro_input!(op_enum as ItemEnum);
    let mut arms = vec![];
    let operations_ident = || Ident::new("Operation", Span::call_site());
    let pattern = |s| {
        Pat::Lit(ExprLit {
            attrs: vec![],
            lit: Lit::Str(LitStr::new(s, Span::call_site())),
        })
    };
    let ident_expr = |s| {
        Expr::Path(ExprPath {
            attrs: vec![],
            qself: None,
            path: Path {
                leading_colon: None,
                segments: Punctuated::from_iter(vec![PathSegment {
                    ident: Ident::new(s, Span::call_site()),
                    arguments: PathArguments::None,
                }]),
            },
        })
    };

    // "q" -> Operation::SaveGraphicsState
    arms.push(Arm {
        pat: pattern("q"),
        guard: None,
        body: Expr::Path(ExprPath {
            attrs: vec![],
            qself: None,
            path: Path {
                leading_colon: None,
                segments: Punctuated::from_iter(vec![
                    PathSegment {
                        ident: operations_ident(),
                        arguments: PathArguments::None,
                    },
                    PathSegment {
                        ident: Ident::new("SaveGraphicsState", Span::call_site()),
                        arguments: PathArguments::None,
                    },
                ]),
            },
        })
        .into(),
        comma: None,
        attrs: vec![],
        fat_arrow_token: Token![=>](Span::call_site()),
    });
    // "Q" -> Operation::RestoreGraphicsState
    arms.push(Arm {
        pat: pattern("Q"),
        guard: None,
        body: Expr::Path(ExprPath {
            attrs: vec![],
            qself: None,
            path: Path {
                leading_colon: None,
                segments: Punctuated::from_iter(vec![
                    PathSegment {
                        ident: operations_ident(),
                        arguments: PathArguments::None,
                    },
                    PathSegment {
                        ident: Ident::new("RestoreGraphicsState", Span::call_site()),
                        arguments: PathArguments::None,
                    },
                ]),
            },
        })
        .into(),
        comma: None,
        attrs: vec![],
        fat_arrow_token: Token![=>](Span::call_site()),
    });
    // "w" => Operation::SetLineWidth(f32::convert_from_object(operands)?)
    arms.push(Arm {
        pat: pattern("w"),
        guard: None,
        body: {
            let convert_call = ExprCall {
                attrs: vec![],
                func: Box::new(Expr::Path(ExprPath {
                    attrs: vec![],
                    qself: None,
                    path: Path {
                        leading_colon: None,
                        segments: Punctuated::from_iter(vec![
                            PathSegment {
                                ident: Ident::new("f32", Span::call_site()),
                                arguments: PathArguments::None,
                            },
                            PathSegment {
                                ident: Ident::new("convert_from_object", Span::call_site()),
                                arguments: PathArguments::None,
                            },
                        ]),
                    },
                })),
                paren_token: Default::default(),
                args: Punctuated::from_iter(vec![ident_expr("operands")]),
            };
            let convert_call = Expr::Try(ExprTry {
                attrs: vec![],
                expr: Box::new(Expr::Call(convert_call)),
                question_token: Default::default(),
            });

            let set_width = Expr::Path(ExprPath {
                attrs: vec![],
                qself: None,
                path: Path {
                    leading_colon: None,
                    segments: Punctuated::from_iter(vec![
                        PathSegment {
                            ident: operations_ident(),
                            arguments: PathArguments::None,
                        },
                        PathSegment {
                            ident: Ident::new("SetLineWidth", Span::call_site()),
                            arguments: PathArguments::None,
                        },
                    ]),
                },
            });

            Expr::Call(ExprCall {
                attrs: vec![],
                func: Box::new(set_width),
                paren_token: Default::default(),
                args: Punctuated::from_iter(vec![convert_call]),
            })
            .into()
        },
        comma: None,
        attrs: vec![],
        fat_arrow_token: Token![=>](Span::call_site()),
    });

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
