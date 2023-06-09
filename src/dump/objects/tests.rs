use super::*;
use crate::old_object::new_plain_ref;
use std::iter::{empty, once, repeat};
use test_case::test_case;

#[test_case(true, None, new_plain_ref(1, 0); "always true if no id")]
#[test_case(true, Some(1), new_plain_ref(1, 0); "true if id matches")]
#[test_case(false, Some(2), new_plain_ref(1, 0); "false if id does not match")]
fn test_equals_to_id(exp: bool, id: Option<u32>, r: PlainRef) {
    assert_eq!(exp, equals_to_id(id, &r));
}

#[test_case(Ok(2), once(2); "one item")]
#[test_case(Err(ExactlyOneError::NoItems), empty(); "no items")]
#[test_case(Err(ExactlyOneError::MoreThanOne), repeat(1); "more than one item")]
fn test_exactly_one(exp: Result<u32, ExactlyOneError>, iter: impl Iterator<Item = u32>) {
    assert_eq!(exp, exactly_one(iter));
}
