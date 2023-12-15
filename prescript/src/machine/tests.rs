use super::*;
use crate::sname;
use assert_approx_eq::assert_approx_eq;
use std::{fmt::Debug, ops::Sub};
use test_log::test;

trait Assert<'a> {
    fn assert(&self, m: &Machine<'a>);
}

macro_rules! ValueEqAssert {
    ($t:ty) => {
        impl<'a> Assert<'a> for $t {
            fn assert(&self, m: &Machine<'a>) {
                assert_eq!(m.stack.len(), 1);
                assert_eq!(m.stack[0], self.clone().into());
            }
        }
    };
}

ValueEqAssert!(bool);
ValueEqAssert!(i32);
ValueEqAssert!(Name);
ValueEqAssert!(RuntimeDictionary<'a>);
ValueEqAssert!(Array);

impl<'a, const N: usize> Assert<'a> for [u8; N] {
    fn assert(&self, m: &Machine<'a>) {
        assert_eq!(m.stack.len(), 1);
        assert_eq!(m.stack[0], (*self).into());
    }
}

impl<'a> Assert<'a> for f32 {
    fn assert(&self, m: &Machine<'a>) {
        assert_eq!(m.stack.len(), 1);
        assert_approx_eq!(m.stack[0].real().unwrap(), *self);
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
fn copy() {
    assert_op(
        "1 2 3 4 5 3 copy",
        Stack(rt_values![1, 2, 3, 4, 5, 3, 4, 5]),
    );
}

#[test]
fn count() {
    assert_op("10 2 count", Stack(rt_values![10, 2, 2]));
}

#[test]
fn test_def() {
    assert_op(
        "10 dict begin /foo 10 def currentdict",
        dict![sname("foo") => 10],
    );
}

#[test]
fn test_end() {
    assert_op(
        "0 dict begin 1 dict begin /foo 10 def end currentdict",
        RuntimeDictionary::new(),
    );
}

#[test]
fn and() {
    // bool
    assert_op("true true and", true);
    assert_op("true false and", false);
    assert_op("false false and", false);
    // int
    assert_op("99 1 and", 1);
    assert_op("52 7 and", 4);
}

#[test]
fn or() {
    // bool
    assert_op("true true or", true);
    assert_op("true false or", true);
    assert_op("false false or", false);
    // int
    assert_op("17 5 or", 21);
}

#[test]
fn not() {
    // bool
    assert_op("true not", false);
    assert_op("false not", true);
    // int
    assert_op("52 not", -53);
}

#[test]
fn xor() {
    // bool
    assert_op("true true xor", false);
    assert_op("true false xor", true);
    assert_op("false false xor", false);
    // int
    assert_op("7 3 xor", 4);
    assert_op("12 3 xor", 15);
}

#[test]
fn eq() {
    assert_op("4 4 eq", true);
    assert_op("4.0 4 eq", true);
    assert_op("4 4.0 eq", true);
    assert_op("(abc) (abc) eq", true);
    assert_op("(abc) /abc eq", true);
    assert_op("/abc (abc) eq", true);
    assert_op("[1 2 3] dup eq", true);
    assert_op("[1 2 3] [1 2 3] eq", false);
    assert_op("{} dup eq", true);
    assert_op("{} {} eq", false);
    assert_op("1 dict 1 dict eq", false);
}

#[test]
fn ne() {
    assert_op("4 4 ne", false);
    assert_op("4.0 4 ne", false);
    assert_op("4 4.0 ne", false);
    assert_op("(abc) (abc) ne", false);
    assert_op("(abc) /abc ne", false);
    assert_op("/abc (abc) ne", false);
    assert_op("[1 2 3] dup ne", false);
    assert_op("[1 2 3] [1 2 3] ne", true);
    assert_op("{} dup ne", false);
    assert_op("{} {} ne", true);
    assert_op("1 dict 1 dict ne", true);
}

#[test]
fn le() {
    assert_op("4 4 le", true);
    assert_op("3 4 le", true);
    assert_op("5 4 le", false);

    assert_op("4.0 4.0 le", true);
    assert_op("3.0 4.0 le", true);
    assert_op("5.0 4.0 le", false);

    assert_op("4.0 4 le", true);
    assert_op("3.0 4 le", true);
    assert_op("5.0 4 le", false);

    assert_op("4 4.0 le", true);
    assert_op("3 4.0 le", true);
    assert_op("5 4.0 le", false);

    assert_op("(4) (4) le", true);
    assert_op("(3) (4) le", true);
    assert_op("(5) (4) le", false);

    assert_op("(4) (40) le", true);
    assert_op("(40) (4) le", false);
}

#[test]
fn lt() {
    assert_op("4 4 lt", false);
    assert_op("3 4 lt", true);
    assert_op("5 4 lt", false);

    assert_op("4.0 4.0 lt", false);
    assert_op("3.0 4.0 lt", true);
    assert_op("5.0 4.0 lt", false);

    assert_op("4.0 4 lt", false);
    assert_op("3.0 4 lt", true);
    assert_op("5.0 4 lt", false);

    assert_op("4 4.0 lt", false);
    assert_op("3 4.0 lt", true);
    assert_op("5 4.0 lt", false);

    assert_op("(4) (4) lt", false);
    assert_op("(3) (4) lt", true);
    assert_op("(5) (4) lt", false);

    assert_op("(4) (40) lt", true);
    assert_op("(40) (4) lt", false);
}

#[test]
fn ge() {
    assert_op("4 4 ge", true);
    assert_op("3 4 ge", false);
    assert_op("5 4 ge", true);

    assert_op("4.0 4.0 ge", true);
    assert_op("3.0 4.0 ge", false);
    assert_op("5.0 4.0 ge", true);

    assert_op("4.0 4 ge", true);
    assert_op("3.0 4 ge", false);
    assert_op("5.0 4 ge", true);

    assert_op("4 4.0 ge", true);
    assert_op("3 4.0 ge", false);
    assert_op("5 4.0 ge", true);

    assert_op("(4) (4) ge", true);
    assert_op("(3) (4) ge", false);
    assert_op("(5) (4) ge", true);

    assert_op("(4) (40) ge", false);
    assert_op("(40) (4) ge", true);
}

#[test]
fn gt() {
    assert_op("4 4 gt", false);
    assert_op("3 4 gt", false);
    assert_op("5 4 gt", true);

    assert_op("4.0 4.0 gt", false);
    assert_op("3.0 4.0 gt", false);
    assert_op("5.0 4.0 gt", true);

    assert_op("4.0 4 gt", false);
    assert_op("3.0 4 gt", false);
    assert_op("5.0 4 gt", true);

    assert_op("4 4.0 gt", false);
    assert_op("3 4.0 gt", false);
    assert_op("5 4.0 gt", true);

    assert_op("(4) (4) gt", false);
    assert_op("(3) (4) gt", false);
    assert_op("(5) (4) gt", true);

    assert_op("(4) (40) gt", false);
    assert_op("(40) (4) gt", true);
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
        dict![sname("foo") => 20],
    );

    // array
    assert_op("2 array dup 1 10 put", values![Value::Null, 10]);
}

#[test]
fn get() {
    // array
    assert_op("2 array dup 1 10 put 1 get", 10);
    // dict
    assert_op("10 dict begin /foo 10 def currentdict /foo get", 10);
    // string
    assert_op("3 string dup 0 65 put 0 get", 65);
    // procedure
    assert_op("{1 2 3} 1 get", 2);
}

#[test]
fn test_for() {
    assert_op("0 1 1 10 {add} for", 55);
}

#[test]
fn test_if() {
    assert_op("true {1} if", 1);
    assert_op("2 false {1} if", 2);
}

#[test]
fn ifelse() {
    assert_op("true {1} {2} ifelse", 1);
    assert_op("false {1} {2} ifelse", 2);
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
fn test_create_dict_on_stack() {
    assert_op("<<>>", dict![]);
    assert_op(
        "<< /foo 10/bar<<>> >>",
        dict![sname("foo") => 10, sname("bar") => dict![]],
    );
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

#[test]
fn to_unicode_cmap() {
    // note: cmap file support not complete!
    // Stub implementation of operations let this specific test pass.
    // kept the code for the time when parse cmap and ToUnicode needed.
    let data = include_bytes!("./to-unicode-cmap");
    let mut machine = Machine::new(data);
    match machine.execute() {
        Ok(_) => {}
        Err(e) => {
            println!("{}:\n{:?}", e, machine.stack);
            panic!();
        }
    }
}

#[test]
fn sub() {
    assert_op("1 2 sub", -1);
    assert_op("1.0 2.0 sub", -1.0);
    assert_op("1.0 2 sub", -1.0);
    assert_op("1 2.0 sub", -1.0);
}

#[test]
fn abs() {
    assert_op("1 abs", 1);
    assert_op("-1 abs", 1);
    assert_op("-1.0 abs", 1.0);
}

#[test]
fn idiv() {
    assert_op("10 3 idiv", 3);
    assert_op("10 -3 idiv", -3);
    assert_op("-10 3 idiv", -3);
    assert_op("-10 -3 idiv", 3);
}

#[test]
fn test_mod() {
    assert_op("10 3 mod", 1);
    assert_op("-5 3 mod", -2);
}

#[test]
fn mul() {
    assert_op("10 3 mul", 30);
    assert_op("-5.0 3.0 mul", -15.0);
    assert_op("-5.0 -3 mul", 15.0);
    assert_op("-5 -3.0 mul", 15.0);
}

#[test]
fn neg() {
    assert_op("10 neg", -10);
    assert_op("-5.0 neg", 5.0);
}

#[test]
fn ceiling() {
    assert_op("3.2 ceiling", 4.0);
    assert_op("-4.8 ceiling", -4.0);
    assert_op("99 ceiling", 99);
}

#[test]
fn floor() {
    assert_op("3.2 floor", 3.0);
    assert_op("-4.8 floor", -5.0);
    assert_op("99 floor", 99);
}

#[test]
fn round() {
    assert_op("3.2 round", 3.0);
    assert_op("-4.8 round", -5.0);
    assert_op("99 round", 99);
}

#[test]
fn truncate() {
    assert_op("3.2 truncate", 3.0);
    assert_op("-4.8 truncate", -4.0);
    assert_op("99 truncate", 99);
}

#[test]
fn sqrt() {
    assert_op("4 sqrt", 2.0);
    assert_op("4.0 sqrt", 2.0);
    assert_op("99 sqrt", 9.949874);
}

#[test]
fn atan() {
    assert_op("0.0 1.0 atan", 0.0);
    assert_op("1 0 atan", 90.0);
    assert_op("-100.0 0 atan", 270.0);
    assert_op("4 4.0 atan", 45.0);
}

#[test]
fn cos() {
    assert_op("0 cos", 1.0);
    assert_op("90.0 cos", 0.0);
    assert_op("180 cos", -1.0);
    assert_op("270.0 cos", 0.0);
}

#[test]
fn sin() {
    assert_op("0 sin", 0.0);
    assert_op("90.0 sin", 1.0);
    assert_op("180 sin", 0.0);
    assert_op("270.0 sin", -1.0);
}

#[test]
fn exp() {
    assert_op("9 0.5 exp", 3.0);
    assert_op("9.0 -1 exp", 0.111111);
}

#[test]
fn ln() {
    assert_op("1 ln", 0.0);
    assert_op("2.0 ln", 0.6931472);
    assert_op("10 ln", 2.3025851);
}
