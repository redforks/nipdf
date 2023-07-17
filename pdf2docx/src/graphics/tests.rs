use super::*;

use test_case::test_case;

#[test_case("w ")]
#[test_case("w"; "not end with whitespace")]
#[test_case("TL ")]
#[test_case("B*\t")]
#[test_case("' "; "quote 1")]
#[test_case("\" "; "quote 2")]
fn parse_operator_succeed(s: &str) {
    let (input, result) = parse_operator(s.as_bytes()).unwrap();
    assert!(input.len() < 2);
    assert_eq!(result, ObjectOrOperator::Operator(s.trim_end()));
}

#[test_case("foo " ; "unknown operator")]
fn parse_operator_falied(s: &str) {
    assert!(matches!(
        parse_operator(s.as_bytes()).unwrap_err(),
        nom::Err::Error(_)
    ));
}

#[test_case(""=> Vec::<Operation>::new(); "empty")]
#[test_case(" % comment\n "=> Vec::<Operation>::new(); "comment only")]
#[test_case(" % comment\n q Q"=> vec![
        Operation::SaveGraphicsState,
        Operation::RestoreGraphicsState
    ];
    "two ops"
)]
fn test_parse_opreations(s: &str) -> Vec<Operation> {
    let (_, result) = parse_operations(s.as_bytes()).unwrap();
    result
}

#[test_case("q" => Operation::SaveGraphicsState; "save")]
#[test_case("Q" => Operation::RestoreGraphicsState; "restore")]
#[test_case("1 w" => Operation::SetLineWidth(1f32))]
#[test_case("1.5 w" => Operation::SetLineWidth(1.5f32))]
#[test_case("1 2 3 1.5 -2 6 cm" => Operation::ModifyCTM(Box::new(TransformMatrix {
    a: 1f32,
    b: 2f32,
    c: 3f32,
    d: 1.5f32,
    e: -2f32,
    f: 6f32,
})); "cm")]
fn test_parse_operation(s: &str) -> Operation {
    let (_, result) = parse_operation(s.as_bytes()).unwrap();
    result
}

#[test_case(0 => LineCapStyle::Butt)]
#[test_case(1 => LineCapStyle::Round)]
#[test_case(2 => LineCapStyle::Square)]
fn test_parse_line_cap_style(v: i32) -> LineCapStyle {
    let mut vec = vec![v.into()];
    let act = LineCapStyle::convert_from_object(&mut vec).unwrap();
    assert!(vec.is_empty());
    act
}
