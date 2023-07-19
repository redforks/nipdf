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
#[test_case("1 2 3 1.5 -2 6 cm" => Operation::ModifyCTM(TransformMatrix {
    a: 1f32,
    b: 2f32,
    c: 3f32,
    d: 1.5f32,
    e: -2f32,
    f: 6f32,
}); "cm")]
#[test_case("[1 2] 0.5 d" => Operation::SetDashPattern(vec![1f32, 2f32], 0.5f32); "dash-pattern")]
#[test_case("/stateName gs" => Operation::SetGraphicsStateParameters(NameOfDict("stateName".into())); "gs")]
#[test_case("1 2 3 4 5 6 c" => Operation::AppendBezierCurve(Point::new(1f32, 2f32), Point::new(3f32, 4f32), Point::new(5f32, 6f32)); "c")]
#[test_case("(foo)Tj" => Operation::ShowText("foo".into()); "Tj")]
#[test_case("[(foo) 1]TJ" => Operation::ShowTexts(vec![TextStringOrNumber::Text("foo".into()), TextStringOrNumber::Number(1f32)]); "show texts")]
#[test_case("/tag /name DP" => Operation::DesignateMarkedContentPointWithProperties(NameOfDict("tag".into()), NameOrDict::Name("name".into())); "DP with name")]
#[test_case("/tag<<>>DP" => Operation::DesignateMarkedContentPointWithProperties(NameOfDict("tag".into()), NameOrDict::Dict(Dictionary::new())); "DP with dict")]
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

#[test_case(vec![] => Vec::<f32>::new(); "empty")]
#[test_case(vec![1f32.into()] => vec![1f32]; "one")]
#[test_case(vec![1f32.into(), 2f32.into()] => vec![1f32, 2f32]; "two")]
fn test_arr_convert_from_object(v: Vec<Object>) -> Vec<f32> {
    let mut outer = vec![Object::Array(v)];
    let act = Vec::<f32>::convert_from_object(&mut outer).unwrap();
    assert!(outer.is_empty());
    act
}

#[test_case(vec![1.into()] => Color::Gray(1.0); "Gray")]
#[test_case(vec![1.into(), 2.0.into(), 3.into()] => Color::Rgb(1.0, 2.0, 3.0); "RGB")]
#[test_case(vec![1.into(), 2.0.into(), 3.into(), 4.0.into()] => Color::Cmyk(1.0, 2.0, 3.0, 4.0); "CMYK")]
#[test_case(vec![1.into(), 2.into(), 3.into(), 4.into(), 5.into()] => Color::Cmyk(2.0, 3.0, 4.0, 5.0); "Max 4 numbers")]
fn color_convert_from_object(mut v: Vec<Object>) -> Color {
    let act = Color::convert_from_object(&mut v).unwrap();
    act
}

#[test]
fn color_convert_from_object_stop_on_non_number() {
    let mut v = vec![true.into(), 1.into()];
    let act = Color::convert_from_object(&mut v).unwrap();
    assert_eq!(act, Color::Gray(1.0));
    assert_eq!(v, vec![true.into()]);
}

#[test_case(vec![1.into()] => ColorOrWithPattern::Color(Color::Gray(1.0)); "Color")]
#[test_case(vec![1.into(), 2.0.into(), 3.into(), "/name".into()] =>
    ColorOrWithPattern::WithPattern(Color::Rgb(1.0, 2.0, 3.0), "name".to_owned()); "With Pattern")]
fn color_or_with_pattern_from_object(mut v: Vec<Object>) -> ColorOrWithPattern {
    let act = ColorOrWithPattern::convert_from_object(&mut v).unwrap();
    act
}
