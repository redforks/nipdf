use super::*;
use test_case::test_case;

#[test_case(Object::Null, "null"; "null")]
#[test_case(true, "true")]
#[test_case(false, "false")]
#[test_case(123, "123"; "integer")]
#[test_case(-123, "-123"; "negative integer")]
#[test_case(123.12, "123.12"; "number")]
#[test_case(-123.12, "-123.12"; "negative number")]
#[test_case(Object::LiteralString(b"()"), "()"; "empty literal string")]
#[test_case(Object::LiteralString(b"(a(foo))"), "(a(foo))"; "nested quoted string")]
#[test_case(Object::LiteralString(b"(a
b)"), "(a
b)"; "literal string contains new line")]
#[test_case(Object::LiteralString(b"(*!&}^%)"), b"(*!&}^%)"; "literal string contains special characters")]
#[test_case(Object::LiteralString(b"(\\)\\()"), b"(\\)\\()"; "literal string contains escape")]
#[test_case(Object::LiteralString(b"(\\333\\n)"), b"(\\333\\n)")]
#[test_case(Object::HexString(b"<>"), b"<>")]
#[test_case(Object::HexString(b"<12A>"), b"<12A>")]
#[test_case(Object::HexString(b"<12 A\t3>"), b"<12 A\t3>"; "contains whitespace")]
fn test_parse_simple_objects(exp: impl Into<Object<'static>>, buf: impl AsRef<[u8]>) {
    assert_eq!(
        (b"".as_slice(), exp.into()),
        parse_object(buf.as_ref()).unwrap()
    );
}
