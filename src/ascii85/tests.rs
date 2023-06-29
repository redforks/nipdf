use super::*;

#[test]
fn test_decode() {
    // extract from sample_files/normal/ASCII85_RunLengthDecode.pdf object 8
    let buf = include_bytes!("./ascii85-1");
    insta::assert_debug_snapshot!(decode(buf).unwrap());
}
