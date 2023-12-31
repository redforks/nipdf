use super::*;
use crate::file::open_test_file_with_password;
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
    let auth = Authorizer {
        revision: StandardHandlerRevision::V2,
        key_length: 40,
        owner_hash,
        user_hash,
        permission_flags: -12i32 as u32,
        doc_id: doc_id.into(),
        encrypt_metadata: true,
    };
    assert!(auth.authorize(b"").is_some());
}

#[test]
fn test_authorize_user_v3() {
    // test case from pdfReferenceUpdated.pdf
    let owner_hash = hex!("63981688733872DEC7983D3C6EB1F412CC535EA2DAA2AB171E2BBC4E36B21887");
    let user_hash = hex!("D64AB15C7434FFE1732E6388274F64C428BF4E5E4E758A4164004E56FFFA0108");
    let doc_id = hex!("9597C618BC90AFA4A078CA72B2DD061C");
    let auth = Authorizer {
        revision: StandardHandlerRevision::V3,
        key_length: 40,
        owner_hash,
        user_hash,
        permission_flags: 0xFFFF_FFE4,
        doc_id: doc_id.into(),
        encrypt_metadata: true,
    };

    assert!(auth.authorize(b"").is_some());
}

#[test]
fn revision_v4_not_encrypt_metadata() -> anyhow::Result<()> {
    let file = open_test_file_with_password("pdf.js/test/pdfs/bug1782186.pdf", "Hello")?;
    let resolver = file.resolver()?;
    let pages = file.catalog(&resolver)?.pages()?;
    assert_eq!(16, pages[0].content()?.operations().len());
    Ok(())
}

#[test]
fn revision_v4() {
    // test case from pdf.js bug1425312.pdf.link
    let owner_hash = hex!("FD2C3D3CE19144D01850580C7870BD45FBA3474163AAC53F0647AD421D4D7030");
    let user_hash = hex!("63369CBE6193F81219F8A12A38B851E928BF4E5E4E758A4164004E56FFFA0108");
    let doc_id = hex!("998F9C0CE1FCCC9E67D0D423081C4C91");
    let auth = Authorizer {
        revision: StandardHandlerRevision::V4,
        key_length: 128,
        owner_hash,
        user_hash,
        permission_flags: 0xFFFF_F2D4,
        doc_id: doc_id.into(),
        encrypt_metadata: true,
    };

    assert!(auth.authorize(b"").is_some());
}
