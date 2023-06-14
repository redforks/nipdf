use super::*;

#[test]
fn resolve_object() {
    let obj = IndirectObject::new(1, 0, b"123");
    assert_eq!(obj.object().unwrap(), &Object::Integer(123));
}