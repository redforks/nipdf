use super::*;

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