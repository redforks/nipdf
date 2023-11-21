use super::*;
use crate::object::HexString;
use test_case::test_case;

#[test_case(Object::Null, "null"; "null")]
#[test_case(true, "true")]
#[test_case(false, "false")]
#[test_case(123, "123"; "integer")]
#[test_case(-123, "-123"; "negative integer")]
#[test_case(123, "+123"; "integer prefixed with +")]
#[test_case(32488685, "32488685"; "integer can not cast from float")]
#[test_case(4294967296f32, "4294967296"; "integer out of range")]
#[test_case(123.12, "123.12"; "number")]
#[test_case(-123.12, "-123.12"; "negative number")]
#[test_case(123.12, "+123.12"; "number prefixed with +")]
#[test_case(4.0, "4.0"; "number end with dot")]
#[test_case(-0.002, "-.002"; "number start with dot")]
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
#[test_case(name!(""), "/"; "empty name")]
#[test_case(name!("foo"), "/foo"; "name")]
fn test_parse_simple_objects(exp: impl Into<Object>, buf: &'static str) {
    let o = parse_object(buf.as_bytes()).unwrap();
    assert_eq!((b"".as_slice(), exp.into()), o);
}

#[test_case(vec![], "[]"; "empty array")]
#[test_case(vec![], "[ \t]"; "empty array 2")]
#[test_case(vec![Object::Null], "[null]"; "array with null")]
#[test_case(vec![Object::Array(vec![Object::Null].into())], "[[null]]"; "nested array with null")]
#[test_case(vec![name!("foo").into()], "[/foo]"; "name value")]
fn test_parse_array(exp: Vec<Object>, buf: &'static str) {
    assert_eq!(
        (b"".as_slice(), exp.into()),
        parse_array(buf.as_bytes()).unwrap()
    );
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
    insta::assert_debug_snapshot!(name, parse_indirect_object(buf.as_ref()).unwrap());
}

#[test_case(b"1 0 R", "simple")]
fn test_parse_reference(buf: impl AsRef<[u8]>, name: &str) {
    insta::assert_debug_snapshot!(name, parse_reference(buf.as_ref()).unwrap());
}

#[test_case("foo", b"/foo")]
#[test_case("a#b", b"/a#23b")]
#[test_case("Ab", b"/#41#62")]
fn name_normalize(exp: impl AsRef<str>, name: impl AsRef<[u8]>) {
    assert_eq!(normalize_name(name.as_ref()).unwrap(), exp.as_ref());
}

#[test]
fn test_parse_object_and_stream() {
    // length is int
    let buf = br#"<</Length 4>>
stream
1234
endstream
"#;
    let (input, o) = parse_object_and_stream(buf).unwrap();
    assert_eq!(input, b"\n");
    let (_, start, length) = o.right().unwrap();
    assert_eq!(21, start);
    assert_eq!(Some(NonZeroU32::new(4).unwrap()), length);

    // length is ref
    let buf = br#"<</Length 1 0 R>>
stream
blah
endstream
"#;
    let (input, o) = parse_object_and_stream(buf).unwrap();
    assert_eq!(input[0], b'b');
    assert!(input.len() > 4);
    let (_, start, length) = o.right().unwrap();
    assert_eq!(25, start);
    assert_eq!(None, length);

    // endstream precede with cr
    let buf = b"<</Length 4>>
stream
1234\rendstream
";
    let (input, o) = parse_object_and_stream(buf).unwrap();
    assert_eq!(input, b"\n");
    let (_, start, length) = o.right().unwrap();
    assert_eq!(21, start);
    assert_eq!(Some(NonZeroU32::new(4).unwrap()), length);

    // length is 0
    let buf = b"<</Length 0>>
stream
endstream
";
    let (input, o) = parse_object_and_stream(buf).unwrap();
    assert_eq!(input, b"\n");
    let (_, start, length) = o.right().unwrap();
    assert_eq!(21, start);
    assert_eq!(None, length);
}
