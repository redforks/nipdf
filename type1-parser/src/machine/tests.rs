use super::*;
use test_log::test;

trait Assert<'a> {
    fn assert(&self, m: &Machine<'a>);
}

impl<'a, V: Into<RuntimeValue<'a>> + Clone> Assert<'a> for V {
    fn assert(&self, m: &Machine<'a>) {
        assert_eq!(m.stack.len(), 1);
        assert_eq!(m.stack[0], self.clone().into());
    }
}

impl<'a> Assert<'a> for Vec<Box<dyn Assert<'a>>> {
    fn assert(&self, m: &Machine<'a>) {
        for a in self {
            a.assert(m);
        }
    }
}

struct Stack<'a>(Vec<RuntimeValue<'a>>);

impl<'a> Assert<'a> for Stack<'a> {
    fn assert(&self, m: &Machine<'a>) {
        assert_eq!(m.stack.len(), self.0.len());
        for (i, v) in self.0.iter().enumerate() {
            assert_eq!(m.stack[i], v.clone());
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
struct VariableStack<'a>(RuntimeDictionary<'a>);

impl<'a> Assert<'a> for VariableStack<'a> {
    fn assert(&self, m: &Machine<'a>) {
        assert_eq!(&*m.variable_stack.top().borrow(), &self.0);
    }
}

fn assert_op<'a>(s: &'a str, exp_result: impl Assert<'a>) {
    let mut machine = Machine::new(s.as_bytes());
    machine.execute().unwrap();
    exp_result.assert(&machine);
}

#[test]
fn test_dict() {
    assert_op("10 dict", RuntimeDictionary::new());
}

#[test]
fn test_begin() {
    assert_op(
        "0 0 dict begin",
        asserts![0, VariableStack(RuntimeDictionary::new())],
    );
}

#[test]
fn test_dup() {
    assert_op("2 dup", Stack(rt_values![2, 2]));
}

#[test]
fn test_def() {
    assert_op("10 dict begin /foo 10 def currentdict", dict!["foo"=> 10]);
}

#[test]
fn test_end() {
    assert_op(
        "0 dict begin 1 dict begin /foo 10 def end currentdict",
        RuntimeDictionary::new(),
    );
}

#[test]
fn test_array() {
    assert_op("2 array", values![Value::Null, Value::Null]);
}

#[test]
fn test_index() {
    assert_op("1 2 3 4 5 3 index", Stack(rt_values![1, 2, 3, 4, 5, 2]));
}

#[test]
fn test_exch() {
    assert_op("3 4 5 exch", Stack(rt_values![3, 5, 4]));
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
    assert_op("1 2 mark 3 4 5 cleartomark", Stack(rt_values![1, 2]));
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
fn array_literal() {
    assert_op("[]", values![]);
}

#[test]
fn known() {
    assert_op("1 dict /foo known", false);
    assert_op("1 dict begin /foo 10 def currentdict end /foo known", true); 
}

#[test]
fn execute_on_file() {
    let data = include_bytes!("./cmsy9.pfb");
    let mut machine = Machine::new(data);
    match machine.execute() {
        Ok(_) => {}
        Err(e) => {
            println!("{}:\n{:?}", e, machine.stack);
            panic!();
        }
    }
}
