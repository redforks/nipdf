use lopdf::StringFormat;

use super::*;

#[test]
fn utf8_or_hex_dumper() {
    let data = b"hello world";
    let dumper = Utf8OrHexDumper(data);
    assert_eq!(format!("{}", dumper), "hello world");

    let data = b"\xf0\x01\x02\x03\x04\x05\x06\x07\x08\t\n\x0b\x0c\r\x0e\x0f";
    let dumper = Utf8OrHexDumper(data);
    assert_eq!(format!("{}", dumper), "0xF00102030405060708090A0B0C0D0E0F");
}

#[test]
fn object_dumper_null() {
    let obj = Object::Null;
    let dumper = ObjectDumper(&obj);
    assert_eq!(format!("{}", dumper), "null");
}

#[test]
fn object_dumper_bool() {
    let obj = Object::Boolean(true);
    let dumper = ObjectDumper(&obj);
    assert_eq!(format!("{}", dumper), "true");

    let obj = Object::Boolean(false);
    let dumper = ObjectDumper(&obj);
    assert_eq!(format!("{}", dumper), "false");
}

#[test]
fn object_dumper_int() {
    let obj = Object::Integer(123);
    let dumper = ObjectDumper(&obj);
    assert_eq!(format!("{}", dumper), "123");
}

#[test]
fn object_dumper_real() {
    let obj = Object::Real(123.456);
    let dumper = ObjectDumper(&obj);
    assert_eq!(format!("{}", dumper), "123.456");
}

#[test]
fn object_dumper_name() {
    let obj = Object::Name(b"hello".to_vec());
    let dumper = ObjectDumper(&obj);
    assert_eq!(format!("{}", dumper), "/hello");
}
