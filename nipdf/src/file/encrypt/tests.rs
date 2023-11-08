use super::*;

#[test]
fn test_pad_trunc_password() {
    // use PADDING if input is empty
    assert_eq!(pad_trunc_password(b""), PADDING);
    // truncate to 32 bytes if input is longer than 32 bytes
    assert_eq!(
        &pad_trunc_password(b"123456789012345678901234567890123"),
        b"12345678901234567890123456789012"
    );
    // pad to 32 bytes if input is shorter than 32 bytes
    assert_eq!(
        &pad_trunc_password(b"1234567890123456789012345678901"),
        b"123456789012345678901234567890\x28\xbf"
    );
}
