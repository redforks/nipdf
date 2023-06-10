use super::*;
use crate::file::Header;

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

#[test]
fn test_riter_lines() {
    // empty
    assert_eq!(0, riter_lines(b"").count());

    // one line without line ending
    assert_eq!(vec![b"hello"], riter_lines(b"hello").collect::<Vec<_>>());

    // one line with line ending
    assert_eq!(vec![b"hello"], riter_lines(b"hello\n").collect::<Vec<_>>());

    // two lines with line ending
    assert_eq!(
        vec![b"world", b"hello"],
        riter_lines(b"hello\n\rworld\r\n").collect::<Vec<_>>()
    );
}

#[test]
fn test_parse_tail() {
    let buf = b"blah\n1234\n%%EOF";
    assert_eq!((b"".as_slice(), Tail::new(1234)), parse_tail(buf).unwrap());
}
