use super::*;
use crate::ParserError;
use test_case::test_case;

#[test_case(b"\n" => b""; "LF")]
#[test_case(b"\r" => b""; "CR")]
#[test_case(b"\r\n" => b""; "CRLF")]
#[test_case(b"\n\r" => b"\r"; "LFCR")]
fn test_eol_3(input: &[u8]) -> &'_ [u8] {
    eol3::<_, ParserError>().parse_peek(input).unwrap().0
}

#[test_case(b"\na" => b"a"; "LF")]
#[test_case(b"\ra" => b"a"; "CR")]
#[test_case(b"\r\na" => b"a"; "CRLF")]
fn test_eol_2(input: &[u8]) -> &'_ [u8] {
    eol2::<_, ParserError>().parse_peek(input).unwrap().0
}

#[test_case(b"%foo\n" => (b"".as_ref(), b"foo".as_ref()); "end with LF")]
#[test_case(b"%foo\r\n" => (b"".as_ref(), b"foo".as_ref()); "end with CRLF")]
#[test_case(b"%\r\n" => (b"".as_ref(), b"".as_ref()); "empty comment")]
#[test_case(b"%foo\r" => (b"".as_ref(), b"foo".as_ref()); "end with CR")]
#[test_case(b"%foo\nbar" => (b"bar".as_ref(), b"foo".as_ref()); "comment with trailing data")]
#[test_case(b"%foo\rbar" => (b"bar".as_ref(), b"foo".as_ref()); "comment with trailing data and CR")]
#[test_case(b"%foo\r\nbar" => (b"bar".as_ref(), b"foo".as_ref()); "comment with trailing data and CRLF")]
#[test_case(b"%" => (b"".as_ref(), b"".as_ref()); "empty comment end with EOF")]
#[test_case(b"%foo" => (b"".as_ref(), b"foo".as_ref()); "comment end with EOF")]
fn test_comment(input: &[u8]) -> (&[u8], &[u8]) {
    comment::<_, ParserError>().parse_peek(input).unwrap()
}

#[test_case(b" " => b""; "space")]
#[test_case(b"\t" => b""; "tab")]
#[test_case(b"\n" => b""; "newline")]
#[test_case(b"\r" => b""; "carriage return")]
#[test_case(b"\x0C" => b""; "form feed")]
fn test_whitespace(input: &[u8]) -> &'_ [u8] {
    whitespace::<_, ParserError>().parse_peek(input).unwrap().0
}
