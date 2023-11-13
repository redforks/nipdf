use super::*;
use crate::parser::{token, ws_prefixed};
use winnow::combinator::iterator;

fn assert_op(s: &str, exp_result: impl Into<Value>) {
    let mut it = iterator(s.as_bytes(), ws_prefixed(token));

    let mut machine = Machine::new();
    machine.execute(&mut it).unwrap();
    assert_eq!(b"", it.finish().unwrap().0);
    assert_eq!(machine.stack.len(), 1);
    assert_eq!(machine.stack[0], exp_result.into());
}

#[test]
fn test_dict() {
    assert_op("10 dict", Dictionary::new());
}
