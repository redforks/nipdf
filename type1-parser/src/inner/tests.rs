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
        parse_header.parse(buf).unwrap()
    );

    let buf = b"%!AdobeFont-1.1: Times-Roman 001.002\n";
    assert_eq!(
        Header {
            spec_ver: "1.1".to_owned(),
            font_name: "Times-Roman".to_owned(),
            font_ver: "001.002".to_owned(),
        },
        parse_header.parse(buf).unwrap()
    );
}
