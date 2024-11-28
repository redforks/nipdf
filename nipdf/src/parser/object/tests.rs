use super::*;
use test_case::test_case;

#[test_case(vec![], "[]"; "empty array")]
#[test_case(vec![], "[ \t]"; "empty array 2")]
#[test_case(vec![Object::Null], "[null]"; "array with null")]
#[test_case(vec![Object::Array(vec![Object::Null].into())], "[[null]]"; "nested array with null")]
#[test_case(vec![sname("foo").into()], "[/foo]"; "name value")]
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
#[test_case(b"1 0 obj 25endobj", "no whitespace between number and endobj")]
#[test_case(
    b"
1 0 obj 25endobj",
    "start with endline"
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
#[test_case("#A5#A5", b"/#A5#A5")]
#[test_case("a#A5#A5", b"/a#A5#A5")]
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
    assert_eq!(Some(4), length);

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
    assert_eq!(Some(4), length);

    // length is 0
    let buf = b"<</Length 0>>
stream
endstream
";
    let (input, o) = parse_object_and_stream(buf).unwrap();
    assert_eq!(input, b"\n");
    let (_, start, length) = o.right().unwrap();
    assert_eq!(21, start);
    assert_eq!(Some(0), length);
}
