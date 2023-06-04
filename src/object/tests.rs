use super::*;

#[test]
fn test_new_name() {
    let name = new_name("test");
    assert_eq!(name, Object::Name(b"test".to_vec()));

    let name = new_name(b"test".as_slice());
    assert_eq!(name, Object::Name(b"test".to_vec()));

    let name = new_name(&"test".to_owned());
    assert_eq!(name, Object::Name(b"test".to_vec()));
}
