use super::*;

#[test]
fn test_equals_to_id() {
    assert!(equals_to_id(None, &(1, 0)));
    assert!(equals_to_id(Some(1), &(1, 0)));
    assert!(!equals_to_id(Some(2), &(1, 0)));
}
