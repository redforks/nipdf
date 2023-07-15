use std::str::from_utf8;

use super::*;
use test_case::test_case;

#[test_case("%foo\n" => "\n"; "end with \n")]
#[test_case("%foo\r\n" => "\r\n"; "end without \r")]
fn test_comment(input: &str) -> &str {
    let (input, v) = comment(input.as_bytes()).unwrap();
    assert_eq!((), v);
    from_utf8(input).unwrap()
}

#[test_case("%PDF-1.7\n"; "PDF version")]
#[test_case("%%EOF\n"; "EOF")]
fn test_comment_exception(input: &str) {
    let _ = comment(input.as_bytes()).unwrap_err();
}

#[test_case("" => ""; "empty")]
#[test_case(" " => ""; "space")]
#[test_case("\t \n\r \0\x0c" => ""; "multiple whitespace")]
#[test_case("% comment" => ""; "comment to the end")]
#[test_case("% comment\nfoo" => "foo"; "comment to eol")]
#[test_case(" % comment\n  % again\r\t bar" => "bar"; "continue comment and whitespaces")]
fn test_whitespace_or_comment(input: &str) -> &str {
    let (input, v) = whitespace_or_comment(input.as_bytes()).unwrap();
    assert_eq!((), v);
    from_utf8(input).unwrap()
}
