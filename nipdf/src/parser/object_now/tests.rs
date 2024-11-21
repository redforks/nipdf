use super::*;
use prescript::sname;
use test_case::test_case;

#[test_case("null" => Object::Null)]
#[test_case("true" => Object::Bool(true))]
#[test_case("false" => Object::Bool(false))]
#[test_case("123" => Object::Integer(123); "integer")]
#[test_case("-123" => Object::Integer(-123); "negative integer")]
#[test_case("+123" => Object::Integer(123); "integer prefixed with +")]
#[test_case("32488685" => Object::Integer(32488685); "integer can not cast from float")]
#[test_case("4294967296" => Object::Number(4294967296f32); "integer out of range")]
#[test_case("123.12" => Object::Number(123.12); "number")]
#[test_case("-123.12" => Object::Number(-123.12); "negative number")]
#[test_case("-" => Object::Number(0.); "negative symbol only")]
#[test_case("+123.12" => Object::Number(123.12); "number prefixed with +")]
#[test_case("4.0" => Object::Number(4.); "number end with dot")]
#[test_case("4.58984938980.04" => Object::Number(4.58984938980); "number ignore 2nd dot")]
#[test_case("-.002" => Object::Number(-0.002); "number start with dot")]
#[test_case("/" => Object::Name(sname("")); "empty name")]
#[test_case("/foo" => Object::Name(sname("foo")); "name")]
#[test_case("/@foo" => Object::Name(sname("@foo")); "special name")]
#[test_case("/foo#20bar" => Object::Name(sname("foo bar")); "contains hex")]
fn parse_object(buf: &str) -> Object {
    object_parser()
        .parse(buf.as_bytes())
        .map_err(|e| snafu::Report::from_error(e.into_inner()))
        .unwrap()
}
