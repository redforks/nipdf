use super::*;
use crate::file::Header;
use insta::assert_debug_snapshot;
use test_case::test_case;

#[test]
fn parse_file_header() {
    let buf = b"%PDF-1.7\n%comment\n";
    let (buf, header) = parse_header(buf).unwrap();
    assert_eq!(buf, b"%comment\n");
    assert_eq!(header, Header::new(b"1.7"));

    let buf = b"%PDF-1.7";
    let err = parse_header(buf);
    assert!(err.is_err());
}

#[test_case(None, b"hello", b"world"; "not exist")]
#[test_case(Some(0), b"hello", b"hello"; "matches")]
#[test_case(Some(1), b"\nhello", b"hello"; "after newline")]
#[test_case(Some(1), b"\nhello\n", b"hello"; "end with newline")]
#[test_case(Some(2), b"\r\nhello\r\n", b"hello"; "CRLF")]
#[test_case(Some(4), b"foo\nfoo\nbar", b"foo"; "from end")]
#[test_case(None, b"abc-foo", b"foo"; "not the whole line")]
fn test_after_tag_r(exp: Option<usize>, buf: &[u8], tag: &[u8]) {
    assert_eq!(exp, r_find_start_object_tag(buf, tag));
}

#[test]
fn test_parse_tail() {
    let buf = b"\nstartxref\n1234\n%%EOF";
    assert_eq!((b"".as_slice(), Tail::new(1234)), parse_tail(buf).unwrap());
}

#[test]
fn test_parse_trailer() {
    let buf = b"trailer\n<< /Size 1 >>\nstartxref\n1234\n%%EOF";
    assert_debug_snapshot!(parse_trailer(buf).unwrap());
}

#[test_case(b"xref\n1 0\n"; "empty")]
#[test_case(b"xref\n1 2\n0000000000 00000 n \n0000000010 00000 n \n"; "two entries")]
fn test_parse_xref_table(buf: impl AsRef<[u8]>) {
    assert_debug_snapshot!(parse_xref_table_section(buf.as_ref()).unwrap());
}
