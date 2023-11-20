use super::*;
use crate::machine::name_token;
use either::{Left, Right};
use test_case::test_case;

macro_rules! tokens {
    () => {
        TokenArray::new()
    };
    ($($e:expr),*) => {
        vec![$(Into::<Token>::into($e)),*]
    }
}

#[test]
fn test_parse_header() {
    let buf = b"%!PS-AdobeFont-1.0: Times-Roman 001.002\n";
    assert_eq!(
        Header {
            spec_ver: "1.0".to_owned(),
            font_name: "Times-Roman".to_owned(),
            font_ver: "001.002".to_owned(),
        },
        header.parse(buf).unwrap()
    );

    let buf = b"%!AdobeFont-1.1: Times-Roman 001.002\n";
    assert_eq!(
        Header {
            spec_ver: "1.1".to_owned(),
            font_name: "Times-Roman".to_owned(),
            font_ver: "001.002".to_owned(),
        },
        header.parse(buf).unwrap()
    );
}

#[test]
fn test_parse_comment() {
    (comment, b'\n').parse(b"% comment\n").unwrap();
    (comment, b'\n').parse(b"%\n").unwrap();
    (comment, b"\r\n").parse(b"%\r\n").unwrap();
    (comment, b'\x0c')
        .parse(b"% end with form feed\x0c")
        .unwrap();
}

#[test_case(b"\n", b"")]
#[test_case(b"\nfoo", b"foo")]
#[test_case(b"\r\n\n", b"\n")]
#[test_case(b"\rfoo", b"foo")]
fn parse_loose_line_ending(buf: &[u8], remains: &[u8]) {
    (loose_line_ending, remains).parse(buf).unwrap();
}

#[test_case(b"1" => Left(1))]
#[test_case(b"123" => Left(123))]
#[test_case(b"-98" => Left(-98))]
#[test_case(b"0" => Left(0))]
#[test_case(b"+17" => Left(17))]
#[test_case(b"-.002" => Right(-0.002))]
#[test_case(b"34.5" => Right(34.5))]
#[test_case(b"-3.62" => Right(-3.62))]
#[test_case(b"123.6e10" => Right(123.6e10))]
#[test_case(b"1.0e-5" => Right(1.0e-5))]
#[test_case(b"-1." => Right(-1.))]
#[test_case(b"0.0" => Right(0.0))]
#[test_case(b"1E6" => Right(1E6))]
#[test_case(b"2e-6" => Right(2e-6))]
#[test_case(b"100000000000" => Right(100000000000_f32))]
#[test_case(b"8#1777" => Left(0o1777))]
#[test_case(b"16#FFFE" => Left(0xFFFE))]
#[test_case(b"2#1000" => Left(0b1000))]
#[test_case(b"36#z" => Left(35))]
fn test_int_or_float(buf: &[u8]) -> Either<i32, f32> {
    int_or_float.parse(buf).unwrap()
}

#[test_case(b"()" => &b""[..]; "empty")]
#[test_case(b"(foo)" => &b"foo"[..])]
#[test_case(b"(foo
new line)" => &b"foo\nnew line"[..])]
#[test_case(b"(&%*<()>)" => &b"&%*<()>"[..]; "nested empty and special symbols")]
#[test_case(b"((a()))" => &b"(a())"[..]; "(a())")]
#[test_case(b"((()b))" => &b"(()b)"[..]; "(()b)")]
#[test_case(b"((a()b))" => &b"(a()b)"[..]; "(a()b)")]
#[test_case(br"(\n\0234\r)" => &b"\n\x134\r"[..]; "escape")]
#[test_case(br"(\0a)" => &b"\0a"[..]; "oct esc 1 byte long")]
#[test_case(br"(\10a)" => &b"\x08a"[..]; "oct esc 2 bytes long")]
#[test_case(br"(\700a)" => &b"\xc0a"[..]; "oct exceed 255 trunc extra bits")]
#[test_case(b"(\\\r)" => &b""[..]; "escaped newline")]
#[test_case(b"<>" => &b""[..]; "empty hex")]
#[test_case(b"<a1>" => &b"\xa1"[..]; "hex one")]
#[test_case(b"<a1b2>" => &b"\xa1\xb2"[..]; "hex two")]
#[test_case(b"< \t>" => &b""[..]; "ignore whitespace")]
#[test_case(b"< a1>" => &b"\xa1"[..]; "ignore whitespace a")]
#[test_case(b"<a1 >" => &b"\xa1"[..]; "ignore whitespace b")]
#[test_case(b"< a1 >" => &b"\xa1"[..]; "ignore whitespace c")]
#[test_case(b"<a1 b>" => &b"\xa1\xb0"[..]; "odd hex length")]
#[test_case(b"<~~>" => &b""[..]; "empty ascii85")]
#[test_case(b"<~!!!!!~>" => &b"\0\0\0\0"[..]; "0 ascii85")]
fn test_string(buf: &[u8]) -> Vec<u8> {
    string.parse(buf).unwrap().to_vec()
}

#[test_case("abc", "" => "abc")]
#[test_case("$$", "" => "$$")]
#[test_case("@pattern", "" => "@pattern")]
#[test_case("a1\t", "\t" => "a1")]
#[test_case("a1(", "(" => "a1")]
fn test_executable_name<'a>(buf: &'a str, remains: &'a str) -> &'a str {
    (executable_name, remains.as_bytes())
        .parse(buf.as_bytes())
        .unwrap()
        .0
}

#[test_case("/(", "(" => ""; "empty")]
#[test_case("/Na$1 ", " " => "Na$1"; "with space")]
#[test_case("/Name/Second", "/Second" => "Name"; "with second")]
#[test_case("/Name(foo)Bar", "(foo)Bar" => "Name"; "with string")]
fn test_literal_name<'a>(buf: &'a str, remains: &'a str) -> &'a str {
    (literal_name, remains.as_bytes())
        .parse(buf.as_bytes())
        .unwrap()
        .0
}

#[test_case("{}"=> tokens![]; "empty")]
#[test_case("{ { } }"=> tokens![tokens![]]; "nested empty")]
#[test_case("{ 10 1.5 ($) [/foo] }"=> tokens![10, 1.5, *b"$", name_token("["), "foo", name_token("]")]; "values")]
fn test_procedure(buf: &str) -> TokenArray {
    procedure.parse(buf.as_bytes()).unwrap()
}

#[test_case("10", "", 10i32)]
#[test_case("10A", "", name_token("10A"))]
fn test_token(buf: &str, remains: &str, exp: impl Into<Token>) {
    assert_eq!(
        exp.into(),
        (token, remains.as_bytes()).parse(buf.as_bytes()).unwrap().0
    );
}
