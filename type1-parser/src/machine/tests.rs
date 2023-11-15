use super::*;
use test_log::test;

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

struct Stack(Vec<Value>);

impl Assert for Stack {
    fn assert(&self, m: &Machine) {
        assert_eq!(m.stack.len(), self.0.len());
        for (i, v) in self.0.iter().enumerate() {
            assert_eq!(&m.stack[i], v);
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
        assert_eq!(&*m.variable_stack.top().borrow(), &self.0);
    }
}

fn assert_op(s: &str, exp_result: impl Assert) {
    let mut machine = Machine::new(s.as_bytes().to_vec());
    machine.execute().unwrap();
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

#[test]
fn test_dup() {
    assert_op("2 dup", Stack(values![2, 2]));
}

#[test]
fn test_def() {
    assert_op("10 dict begin /foo 10 def currentdict", dict!["foo"=> 10]);
}

#[test]
fn test_end() {
    assert_op(
        "0 dict begin 1 dict begin /foo 10 def end currentdict",
        Dictionary::new(),
    );
}

#[test]
fn test_array() {
    assert_op("2 array", values![Value::Null, Value::Null]);
}

#[test]
fn test_index() {
    assert_op("1 2 3 4 5 3 index", Stack(values![1, 2, 3, 4, 5, 2]));
}

#[test]
fn test_exch() {
    assert_op("3 4 5 exch", Stack(values![3, 5, 4]));
}

#[test]
fn test_put() {
    // dict
    assert_op(
        "10 dict begin /foo 10 def currentdict /foo 20 put currentdict",
        dict!["foo"=> 20],
    );

    // array
    assert_op("2 array dup 1 10 put", values![Value::Null, 10]);
}

#[test]
fn test_for() {
    assert_op("0 1 1 10 {add} for", 55);
}

#[test]
fn test_cleartomark() {
    assert_op("1 2 mark 3 4 5 cleartomark", Stack(values![1, 2]));
}

#[test]
fn test_create_array_on_stack() {
    assert_op("[ 1 2 3 4 5 ]", values![1, 2, 3, 4, 5]);
}

#[test]
fn test_string() {
    assert_op("3 string", *b"\0\0\0");
}

#[test]
fn execute_on_file() {
    let data = include_bytes!("./cmsy9.pfb");
    let mut machine = Machine::new(data.to_vec());
    match machine.execute() {
        Ok(_) => {}
        Err(e) => {
            println!("{}:\n{:?}", e, machine.stack);
            panic!();
        }
    }
}

#[test]
fn pfb_file() {
    let data = include_bytes!("../../../nipdf/fonts/d050000l.pfb");
    let mut machine = Machine::new(data.to_vec());
    match machine.execute() {
        Ok(_) => {}
        Err(e) => {
            println!("{}:\n{:?}", e, machine.stack);
            panic!();
        }
    }
}
