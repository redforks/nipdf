use super::*;
use crate::ParserError;
use std::{
    any::{Any, TypeId},
    str::from_utf8,
};
use test_case::test_case;
use winnow::error::ErrMode;

#[test_case("%foo\n" => "\n"; "end with \n")]
#[test_case("%foo\r\n" => "\r\n"; "end without \r")]
#[test_case("%\r\n" => "\r\n"; "empty comment")]
fn test_comment(input: &str) -> &str {
    let (input, v) = comment(input.as_bytes()).unwrap();
    fn is_unit<T: ?Sized + Any>(_v: &T) -> bool {
        TypeId::of::<()>() == TypeId::of::<T>()
    }
    assert!(is_unit(&v));
    from_utf8(input).unwrap()
}

#[test_case("%PDF-1.7\n"; "PDF version")]
#[test_case("%%EOF\n"; "EOF")]
fn test_comment_exception(input: &str) {
    let _ = comment(input.as_bytes()).unwrap_err();
}

#[test_case("" => ""; "empty")]
#[test_case(" " => ""; "space")]
#[test_case("\t \n\r \x0c" => ""; "multiple whitespace")]
#[test_case("% comment" => ""; "comment to the end")]
#[test_case("% comment\nfoo" => "foo"; "comment to eol")]
#[test_case(" % comment\n  % again\r\t bar" => "bar"; "continue comment and white spaces")]
fn test_whitespace_or_comment(input: &str) -> &str {
    let (input, _): (_, ()) = whitespace_or_comment(input.as_bytes()).unwrap();
    from_utf8(input).unwrap()
}

#[test_case(b"\n" => b""; "LF")]
#[test_case(b"\r" => b""; "CR")]
#[test_case(b"\r\n" => b""; "CRLF")]
#[test_case(b"\n\r" => b"\r"; "LFCR")]
fn test_eol_3(input: &[u8]) -> &'_ [u8] {
    eol3::<_, ParserError>().parse_peek(input).unwrap().0
}

#[test_case(b"\n" => b""; "LF")]
#[test_case(b"\r\n" => b""; "CRLF")]
fn test_eol_2(input: &[u8]) -> &'_ [u8] {
    eol2::<_, ParserError>().parse_peek(input).unwrap().0
}

#[test]
fn test_eol_2_cr() {
    let e = eol2::<_, ParserError>()
        .parse_next(&mut b"\r".as_ref())
        .unwrap_err();
    assert!(matches!(e, ErrMode::<ParserError>::Backtrack(_)))
}

#[test_case(b"%foo\n" => (b"".as_ref(), b"foo".as_ref()); "end with LF")]
#[test_case(b"%foo\r\n" => (b"".as_ref(), b"foo".as_ref()); "end with CRLF")]
#[test_case(b"%\r\n" => (b"".as_ref(), b"".as_ref()); "empty comment")]
#[test_case(b"%foo\r" => (b"".as_ref(), b"foo".as_ref()); "end with CR")]
#[test_case(b"%foo\nbar" => (b"bar".as_ref(), b"foo".as_ref()); "comment with trailing data")]
#[test_case(b"%foo\rbar" => (b"bar".as_ref(), b"foo".as_ref()); "comment with trailing data and CR")]
#[test_case(b"%foo\r\nbar" => (b"bar".as_ref(), b"foo".as_ref()); "comment with trailing data and CRLF")]
fn test_comment_now(input: &[u8]) -> (&[u8], &[u8]) {
    comment_now::<_, ParserError>().parse_peek(input).unwrap()
}

#[test_case(b" " => b""; "space")]
#[test_case(b"\t" => b""; "tab")]
#[test_case(b"\n" => b""; "newline")]
#[test_case(b"\r" => b""; "carriage return")]
#[test_case(b"\x0C" => b""; "form feed")]
fn test_whitespace_now(input: &[u8]) -> &'_ [u8] {
    whitespace_now::<_, ParserError>()
        .parse_peek(input)
        .unwrap()
        .0
}
