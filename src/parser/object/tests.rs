use super::*;
use test_case::test_case;

#[test_case(Object::Null, "null"; "null")]
#[test_case(Object::Bool(true), "true")]
#[test_case(Object::Bool(false), "false")]
#[test_case(Object::Integer(123), "123"; "integer")]
#[test_case(Object::Integer(-123), "-123"; "negative integer")]
// #[test_case(Object::Number(123.0), "123.0"; "number")]
// #[test_case(Object::Number(-123.0), "-123.0"; "negative number")]
fn test_parse_simple_objects(exp: Object, buf: impl AsRef<[u8]>) {
    assert_eq!((b"".as_slice(), exp), parse_object(buf.as_ref()).unwrap());
}
