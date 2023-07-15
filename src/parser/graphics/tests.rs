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
