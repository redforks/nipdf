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
#[test_case(Name::new(b"/"), b"/"; "empty name")]
#[test_case(Name::new(b"/foo"), b"/foo"; "name")]
fn test_parse_simple_objects(exp: impl Into<Object<'static>>, buf: impl AsRef<[u8]>) {
    assert_eq!(
        (b"".as_slice(), exp.into()),
        parse_object(buf.as_ref()).unwrap()
    );
}

#[test_case(vec![], b"[]"; "empty array")]
#[test_case(vec![], b"[ \t]"; "empty array 2")]
#[test_case(vec![Object::Null], b"[null]"; "array with null")]
#[test_case(vec![Object::Array(vec![Object::Null])], b"[[null]]"; "nested array with null")]
fn test_parse_array(exp: Vec<Object<'static>>, buf: impl AsRef<[u8]>) {
    assert_eq!(
        (b"".as_slice(), exp.into()),
        parse_array(buf.as_ref()).unwrap()
    );
}

#[test_case(vec![], b"<< >>"; "empty dict")]
#[test_case(vec![(Name::new(b"/Type"), Name::new(b"/Catalog").into())], b"<< /Type /Catalog >>"; "dict with one entry")]
fn test_parse_dict(exp: Vec<(Name, Object)>, buf: impl AsRef<[u8]>) {
    assert_eq!(
        parse_dict(buf.as_ref()),
        Ok((&[][..], exp.into_iter().collect()))
    );
}

#[test_case(
    b"<</Length 0>>
stream
endstream"; "empty"
)]
#[test_case(
    b"<</Length 4>>
stream
abcd
endstream"; "not empty"
)]
fn test_parse_stream(buf: impl AsRef<[u8]>) {
    insta::assert_debug_snapshot!(parse_stream(buf.as_ref()).unwrap());
}
