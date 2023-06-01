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
