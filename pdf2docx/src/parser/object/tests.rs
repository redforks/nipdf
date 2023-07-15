

use super::*;
use crate::object::HexString;
use test_case::test_case;

#[test_case(Object::Null, "null"; "null")]
#[test_case(true, "true")]
#[test_case(false, "false")]
#[test_case(123, "123"; "integer")]
#[test_case(-123, "-123"; "negative integer")]
#[test_case(123.12, "123.12"; "number")]
#[test_case(-123.12, "-123.12"; "negative number")]
#[test_case(LiteralString::new(b"()"), "()"; "empty literal string")]
#[test_case(LiteralString::new(b"(5\\()"), "(5\\()"; "escaped )")]
#[test_case(LiteralString::new(b"(5\\\\)"), "(5\\\\)"; "escaped back slash")]
#[test_case(LiteralString::new(b"(a(foo))"), "(a(foo))"; "nested quoted string")]
#[test_case(LiteralString::new(b"(a
b)"), "(a
b)"; "literal string contains new line")]
#[test_case(LiteralString::new(b"(*!&}^%)"), "(*!&}^%)"; "literal string contains special characters")]
#[test_case(LiteralString::new(b"(\\)\\()"), "(\\)\\()"; "literal string contains escape")]
#[test_case(LiteralString::new(b"(\\333\\n)"), "(\\333\\n)")]
#[test_case(HexString::new(b"<>"), "<>")]
#[test_case(HexString::new(b"<12A>"), "<12A>")]
#[test_case(HexString::new(b"<12 A\t3>"), "<12 A\t3>"; "contains whitespace")]
#[test_case(Name::borrowed(b""), "/"; "empty name")]
#[test_case(Name::borrowed(b"foo"), "/foo"; "name")]
fn test_parse_simple_objects(exp: impl Into<Object<'static>>, buf: &'static str) {
    let o = parse_object(buf.as_bytes()).unwrap();
    assert_eq!((b"".as_slice(), exp.into()), o);
}

#[test_case(vec![], "[]"; "empty array")]
#[test_case(vec![], "[ \t]"; "empty array 2")]
#[test_case(vec![Object::Null], "[null]"; "array with null")]
#[test_case(vec![Object::Array(vec![Object::Null])], "[[null]]"; "nested array with null")]
#[test_case(vec![Name::borrowed(b"foo").into()], "[/foo]"; "name value")]
fn test_parse_array(exp: Vec<Object<'static>>, buf: &'static str) {
    assert_eq!((b"".as_slice(), exp), parse_array(buf.as_bytes()).unwrap());
}

#[test_case(b"<< >>", "empty dict")]
#[test_case(b"<<>>", "empty dict 2")]
#[test_case(b"<< /Type /Catalog >>", "dict with one entry")]
#[test_case(b"<</Inner<<>>>>", "nested")]
#[test_case(b"<</id[]>>", "empty array")]
#[test_case(b"<</id()>>", "string value")]
#[test_case(b"<</id/Value>>", "name value")]
#[test_case(b"<</id/>>", "empty name value")]
#[test_case(b"<<//id>>", "empty name key")]
#[test_case(b"<</id<<>>>>", "nested empty dict")]
fn test_parse_dict(buf: impl AsRef<[u8]>, name: &str) {
    insta::assert_debug_snapshot!(name, parse_dict(buf.as_ref()).unwrap());
}

#[test_case(
    b"1 0 obj
null
endobj",
    "null"
)]
fn test_parse_indirected_object(buf: impl AsRef<[u8]>, name: &str) {
    insta::assert_debug_snapshot!(name, parse_indirected_object(buf.as_ref()).unwrap());
}

#[test_case(b"1 0 R", "simple")]
fn test_parse_reference(buf: impl AsRef<[u8]>, name: &str) {
    insta::assert_debug_snapshot!(name, parse_reference(buf.as_ref()).unwrap());
}

#[test_case(b"foo", b"/foo")]
#[test_case(b"a#b", b"/a#23b")]
#[test_case(b"Ab", b"/#41#62")]
fn name_normalize(exp: impl AsRef<[u8]>, name: impl AsRef<[u8]>) {
    assert_eq!(normalize_name(name.as_ref()).unwrap(), exp.as_ref());
}
