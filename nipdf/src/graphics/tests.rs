use super::*;
use crate::object::LiteralString;
use prescript::sname;
use test_case::test_case;

#[test_case("w ", "w")]
#[test_case("w", "w"; "not end with whitespace")]
#[test_case("TL ", "TL")]
#[test_case("B*\t", "B*")]
#[test_case("' ", "'"; "quote 1")]
#[test_case("\" ", "\""; "quote 2")]
#[test_case("Tc[", "Tc"; "end with separator 1")]
#[test_case("Tc<", "Tc"; "end with separator 2")]
#[test_case("Tc(", "Tc"; "end with separator 3")]
#[test_case("q/foo", "q"; "end with Name")]
fn parse_operator_succeed(s: &str, op: &str) {
    let len = s.len();
    let (input, result) = parse_operator(s.as_bytes()).unwrap();
    assert_eq!(input.len() + op.len(), len);
    assert_eq!(result, ObjectOrOperator::Operator(op));
}

#[test_case(""=> Vec::<Operation>::new(); "empty")]
#[test_case(" % comment\n "=> Vec::<Operation>::new(); "comment only")]
#[test_case(" % comment\n q Q"=> vec![
        Operation::SaveGraphicsState,
        Operation::RestoreGraphicsState
    ];
    "two ops"
)]
#[test_case("q 296.000000 0 0 295.000000 0 0 cm/Image80 Do Q " => vec![
        Operation::SaveGraphicsState,
        Operation::ModifyCTM(UserToUserSpace::new(
            296f32, 0f32, 0f32, 295f32, 0f32, 0f32
        )),
        Operation::PaintXObject(NameOfDict(sname("Image80"))),
        Operation::RestoreGraphicsState
    ];
    "cm and Do"
)]
fn test_parse_operations(s: &str) -> Vec<Operation> {
    let (_, result) = parse_operations(s.as_bytes()).unwrap();
    result
}

#[test_case("q" => Operation::SaveGraphicsState; "save")]
#[test_case("Q" => Operation::RestoreGraphicsState; "restore")]
#[test_case("1 w" => Operation::SetLineWidth(1f32))]
#[test_case("1.5 w" => Operation::SetLineWidth(1.5f32))]
#[test_case("296.000000 0 0 295.000000 0 0 cm" => Operation::ModifyCTM(
    UserToUserSpace::new(296f32, 0f32, 0f32, 295f32, 0f32, 0f32)
); "cm")]
#[test_case("[1 2] 0.5 d" => Operation::SetDashPattern(vec![1f32, 2f32], 0.5f32); "dash-pattern")]
#[test_case("/stateName gs" => Operation::SetGraphicsStateParameters(NameOfDict(sname("stateName"))); "gs")]
#[test_case("1 2 3 4 5 6 c" => Operation::AppendBezierCurve(Point::new(1f32, 2f32), Point::new(3f32, 4f32), Point::new(5f32, 6f32)); "c")]
#[test_case("(foo)Tj" => Operation::ShowText(TextString::Text(LiteralString::new(b"(foo)"))); "Tj")]
#[test_case("[(foo) 1]TJ" => Operation::ShowTexts(vec![TextStringOrNumber::TextString(TextString::Text(LiteralString::new(b"(foo)"))), TextStringOrNumber::Number(Length::new(1f32))]); "show texts")]
#[test_case("/tag /name DP" => Operation::DesignateMarkedContentPointWithProperties(NameOfDict(sname("tag")), NameOrDict::Name(sname("name"))); "DP with name")]
#[test_case("/tag<<>>DP" => Operation::DesignateMarkedContentPointWithProperties(NameOfDict(sname("tag")), NameOrDict::Dict(Dictionary::new())); "DP with dict")]
fn test_parse_operation(s: &str) -> Operation {
    let (_, mut result) = parse_operations(s.as_bytes()).unwrap();
    assert_eq!(1, result.len());
    result.pop().unwrap()
}

#[test]
fn test_ignore_bx_ex() {
    let (buf, result) = parse_operations(b"BX\nq\nEX\nQ").unwrap();
    assert_eq!(buf, b"");
    assert_eq!(
        vec![
            Operation::SaveGraphicsState,
            Operation::RestoreGraphicsState
        ],
        result
    );
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
    let mut outer = vec![Object::Array(v.into())];
    let act = Vec::<f32>::convert_from_object(&mut outer).unwrap();
    assert!(outer.is_empty());
    act
}

#[test_case(vec![1.into()] => ColorArgsOrName::Color(ColorArgs(vec![1.0])); "Color")]
#[test_case(vec!["/name".into()] => ColorArgsOrName::Name((sname("name"), None)); "name")]
#[test_case(vec![1f32.into(), 2f32.into(), 3f32.into(), "/p1".into()] => ColorArgsOrName::Name((sname("p1"), Some(ColorArgs(vec![1f32, 2., 3.])))); "SCN for uncolored pattern")]
fn color_or_with_pattern_from_object(mut v: Vec<Object>) -> ColorArgsOrName {
    ColorArgsOrName::convert_from_object(&mut v).unwrap()
}

#[test]
fn transform_try_from_array() {
    use euclid::default::Transform2D;
    let arr = vec![1.into(), 2.into(), 3.into(), 4.into(), 5.into(), 6.into()];
    let o = arr.into();
    let act = Transform2D::try_from(&o).unwrap();
    assert_eq!(act, Transform2D::new(1f32, 2f32, 3f32, 4f32, 5f32, 6f32));
}
