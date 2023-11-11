use super::*;
use hex_literal::hex;

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
        &pad_trunc_password(b"123456789012345678901234567890"),
        b"123456789012345678901234567890\x28\xbf"
    );
}

#[test]
fn test_authorize_user_v2() {
    // test case from 5177.Type2.pdf
    let owner_hash = hex!("02d8a74390d98f0ca54f6e9fb9e607ced6f67f8c436a748d9add5bb883e5795d");
    let user_hash = hex!("3dd77b55ad61a0f5d7bce45989272a151797e20b8333013f8f9b94c7e95bd68c");
    let doc_id = hex!("9f564085cf0a9f7c1145bf1163e68212");
    assert!(authorize_user(
        StandardHandlerRevion::V2,
        40,
        b"",
        &owner_hash,
        &user_hash,
        unsafe { std::mem::transmute(-12i32) },
        &doc_id,
    ));
}

#[test]
fn test_authorize_user_v3() {
    // test case from pdfReferenceUpdated.pdf
    let owner_hash = hex!("63981688733872DEC7983D3C6EB1F412CC535EA2DAA2AB171E2BBC4E36B21887");
    let user_hash = hex!("D64AB15C7434FFE1732E6388274F64C428BF4E5E4E758A4164004E56FFFA0108");
    let doc_id = hex!("9597C618BC90AFA4A078CA72B2DD061C");
    assert!(authorize_user(
        StandardHandlerRevion::V3,
        40,
        b"",
        &owner_hash,
        &user_hash,
        0xFFFF_FFE4,
        &doc_id,
    ));
}
