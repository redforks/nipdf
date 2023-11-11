use super::*;
use either::{Left, Right};
use test_case::test_case;

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
