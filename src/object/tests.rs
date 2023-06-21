use super::*;
use test_case::test_case;

#[test_case(b"", b"<>" ; "empty")]
#[test_case(b"\x90\x1f\xa3", b"<901FA3>"; "not empty")]
#[test_case(b"\x90\x1f\xa0", b"<901FA>"; "append 0 if odd")]
#[test_case(b"\x90\x1f\xa0", b"<90 1F\tA>"; "ignore whitespace")]
fn as_hex_string(exp: impl AsRef<[u8]>, buf: impl AsRef<[u8]>) {
    assert_eq!(
        Object::HexString(buf.as_ref()).as_hex_string().unwrap(),
        exp.as_ref()
    );
}
