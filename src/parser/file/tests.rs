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
