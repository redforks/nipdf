use super::*;
use crate::parser::{token, ws_prefixed};
use winnow::combinator::iterator;

trait Assert {
    fn assert(&self, m: &Machine);
}

/// Assert that Machine stack length is 1 and equals to the given value
impl<V: Into<Value> + Clone> Assert for V {
    fn assert(&self, m: &Machine) {
        assert_eq!(m.stack.len(), 1);
        assert_eq!(m.stack[0], self.clone().into());
    }
}

impl Assert for Vec<Box<dyn Assert>> {
    fn assert(&self, m: &Machine) {
        for a in self {
            a.assert(m);
        }
    }
}

macro_rules! asserts {
    ($($e:expr),*) => {
        vec![$(Box::new($e) as Box<dyn Assert>),*]
    }
}

/// Check Dict stack current top equals to the given value
#[derive(Clone)]
struct VariableStack(Dictionary);

impl Assert for VariableStack {
    fn assert(&self, m: &Machine) {
        assert_eq!(m.variable_stack.top(), &self.0);
    }
}

fn assert_op(s: &str, exp_result: impl Assert) {
    let mut it = iterator(s.as_bytes(), ws_prefixed(token));

    let mut machine = Machine::new();
    machine.execute(&mut it).unwrap();
    assert_eq!(b"", it.finish().unwrap().0);
    exp_result.assert(&machine);
}

#[test]
fn test_dict() {
    assert_op("10 dict", Dictionary::new());
}

#[test]
fn test_begin() {
    assert_op(
        "0 0 dict begin",
        asserts![0, VariableStack(Dictionary::new())],
    );
}
