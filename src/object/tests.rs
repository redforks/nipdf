use super::*;
use test_case::test_case;

#[test]
fn as_string_non_string() {
    // not string object
    assert_eq!(
        Object::Null.as_string().unwrap_err(),
        ObjectValueError::UnexpectedType
    );
}

#[test_case("", b"()"; "empty")]
#[test_case("a", b"(a)"; "single character")]
#[test_case("a(", b"(a\\()"; "left square")]
#[test_case("a)", b"(a\\))"; "right square")]
#[test_case("ab", b"(a\\\nb)"; "escape next \\n")]
#[test_case("ab", b"(a\\\rb)"; "escape next \\r")]
#[test_case("ab", b"(a\\\r\nb)"; "escape next \\r\\n")]
#[test_case("ab", b"(a\\\n\rb)"; "escape next \\n\\r")]
#[test_case("a\nb", b"(a\\\n\nb)"; "escape one next new line")]
#[test_case("a\nb", b"(a\nb)"; "normal new line")]
#[test_case("a\nb", b"(a\rb)"; "normal \\n new line")]
#[test_case("a\nb", b"(a\r\nb)"; "normal \\r\\n new line")]
#[test_case("a\nb", b"(a\n\rb)"; "normal \\n\\r new line")]
#[test_case("\x05a", b"(\\5a)"; "oct 1")]
#[test_case("\x05a", b"(\\05a)"; "oct 2")]
#[test_case("\x05a", b"(\\005a)"; "oct 3")]
fn literal_string_as_string(exp: &str, buf: impl AsRef<[u8]>) {
    assert_eq!(
        Object::LiteralString(buf.as_ref()).as_string().unwrap(),
        exp
    );
}

#[test_case(b"", b"<>" ; "empty")]
#[test_case(b"\x90\x1f\xa3", b"<901FA3>"; "not empty")]
#[test_case(b"\x90\x1f\xa0", b"<901FA>"; "append 0 if odd")]
#[test_case(b"\x90\x1f\xa0", b"<90 1F\tA>"; "ignore whitespace")]
fn as_hex_string(exp: impl AsRef<[u8]>, buf: impl AsRef<[u8]>) {
    assert_eq!(
        Object::HexString(buf.as_ref()).as_hex_string().unwrap(),
        exp.as_ref()
    );
}

#[test_case(true, b"/foo", b"/foo")]
#[test_case(false, b"/foo", b"/bar")]
#[test_case(false, b"/foo", b"/Foo")]
#[test_case(true, b"/#46oo", b"/Foo")]
fn name_equals(exp: bool, name1: impl AsRef<[u8]>, name2: impl AsRef<[u8]>) {
    assert_eq!(Name(name1.as_ref()) == Name(name2.as_ref()), exp);
}

#[test_case(b"foo", b"/foo")]
#[test_case(b"a#b", b"/a#23b")]
#[test_case(b"Ab", b"/#41#62")]
fn name_normalize(exp: impl AsRef<[u8]>, name: impl AsRef<[u8]>) {
    assert_eq!(Name(name.as_ref()).normalize().unwrap(), exp.as_ref());
}

#[test]
fn as_name() {
    assert_eq!(
        Object::Name(Name(b"/foo")).as_name().unwrap().as_ref(),
        &b"foo"[..]
    );

    assert_eq!(
        Object::LiteralString(b"(foo)").as_name().unwrap_err(),
        ObjectValueError::UnexpectedType
    );
}
