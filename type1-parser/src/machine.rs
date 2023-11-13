use educe::Educe;
use std::{collections::HashMap, rc::Rc};

pub type Array = Vec<Value>;
pub type TokenArray = Vec<Token>;

type OperatorFn = fn(&mut Machine) -> MachineResult<()>;

#[derive(Educe)]
#[educe(Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Integer(i32),
    Real(f32),
    String(Rc<[u8]>),
    Array(Rc<Array>),
    Dictionary(Rc<Dictionary>),
    Procedure(Rc<TokenArray>),
    Name(String),
    BuiltInOp(
        #[educe(Debug(ignore))]
        #[educe(PartialEq(ignore))]
        OperatorFn,
    ),
}

/// Type of `Dictionary` key. PostScript allows any value to be key except null,
/// String will convert to Name when used as key.
/// But I don't want to implement that, so I will only allow `Bool`, `Integer`,
/// and `Name`, and convert `String` to `Name` when used as key.
/// We'll figure it out if encounter other types, it should not happen in practice.
#[derive(Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Bool(bool),
    Integer(i32),
    Name(String),
}

#[derive(Debug, PartialEq, Educe)]
#[educe(Deref, Default(new))]
pub struct Dictionary(HashMap<Key, Value>);

impl Dictionary {
    pub fn with_capacity(capacity: usize) -> Self {
        Self(HashMap::with_capacity(capacity))
    }
}

#[derive(Debug, PartialEq)]
pub enum Token {
    Literal(Value),
    /// Name to lookup operation dict to get the actual operator
    Name(String),
}

pub fn name_token(s: impl Into<String>) -> Token {
    Token::Name(s.into())
}

impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::Integer(v)
    }
}

impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Value::Real(v)
    }
}

impl<const N: usize> From<[u8; N]> for Value {
    fn from(v: [u8; N]) -> Self {
        let v: Box<[u8]> = v.into();
        v.into()
    }
}

impl From<Box<[u8]>> for Value {
    fn from(v: Box<[u8]>) -> Self {
        Value::String(v.into())
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::Name(v.to_owned())
    }
}

impl From<Array> for Value {
    fn from(v: Array) -> Self {
        Value::Array(v.into())
    }
}

impl From<TokenArray> for Value {
    fn from(v: TokenArray) -> Self {
        Value::Procedure(v.into())
    }
}

impl From<Dictionary> for Value {
    fn from(v: Dictionary) -> Self {
        Value::Dictionary(v.into())
    }
}

impl<T: Into<Value>> From<T> for Token {
    fn from(v: T) -> Self {
        Token::Literal(v.into())
    }
}

impl From<OperatorFn> for Value {
    fn from(v: OperatorFn) -> Self {
        Value::BuiltInOp(v)
    }
}

/// Create Array from a list of values that implement Into<Object> trait
macro_rules! array {
    () => {
        Array::new()
    };
    ($($e:expr),*) => {
        vec![$(Into::<Value<'static>>::into($e)),*]
    }
}
pub(crate) use array;

macro_rules! tokens {
    () => {
        TokenArray::new()
    };
    ($($e:expr),*) => {
        vec![$(Into::<Token>::into($e)),*]
    }
}
pub(crate) use tokens;

#[derive(Debug, PartialEq, thiserror::Error)]
pub enum MachineError {
    #[error("stack underflow")]
    StackUnderflow,
    #[error("stack overflow")]
    StackOverflow,
    #[error("type check error")]
    TypeCheck,
    #[error("undefined")]
    Undefined,
    #[error("unimplemented")]
    Unimplemented,
}

pub type MachineResult<T> = Result<T, MachineError>;

/// PostScript machine to execute operations.
pub struct Machine {
    variable_stack: VariableDictStack,
    stack: Vec<Value>,
}

impl Machine {
    pub fn new() -> Self {
        Self {
            variable_stack: VariableDictStack::new(),
            stack: Vec::new(),
        }
    }

    pub fn execute(&mut self, tokens: impl IntoIterator<Item = Token>) -> MachineResult<()> {
        for token in tokens {
            match token {
                Token::Literal(v) => self.push(v),
                Token::Name(name) => {
                    let v = self.variable_stack.get(&name)?;
                    match v {
                        Value::BuiltInOp(op) => op(self)?,
                        _ => unreachable!(),
                    }
                }
            }
        }
        Ok(())
    }

    fn pop(&mut self) -> MachineResult<Value> {
        self.stack.pop().ok_or(MachineError::StackUnderflow)
    }

    fn pop_int(&mut self) -> MachineResult<i32> {
        self.pop().and_then(|v| match v {
            Value::Integer(i) => Ok(i),
            _ => Err(MachineError::TypeCheck),
        })
    }

    fn push(&mut self, v: impl Into<Value>) {
        self.stack.push(v.into());
    }
}

type VariableDict = HashMap<String, Value>;

struct VariableDictStack {
    stack: Vec<VariableDict>,
}

macro_rules! var_dict {
    ($($k:expr => $v:expr),* $(,)?) => {{
        use std::iter::{Iterator, IntoIterator};
        Iterator::collect(IntoIterator::into_iter([$(($k.to_owned(), $v.into()),)*]))
    }};
}

/// Create the `systemdict`
fn system_dict() -> VariableDict {
    let f: OperatorFn = dict;
    var_dict!(
        "dict" => f
    )
}

/// Create the `globaldict`
fn global_dict() -> VariableDict {
    let mut dict = VariableDict::new();
    // dict.insert("globaldict".to_owned(), Value::Dictionary(Rc::new(dict)));
    dict
}

/// Create the `userdict`
fn user_dict() -> VariableDict {
    let mut dict = VariableDict::new();
    // dict.insert("userdict".to_owned(), Value::Dictionary(Rc::new(dict)));
    dict
}

impl VariableDictStack {
    fn new() -> Self {
        Self {
            stack: vec![system_dict(), global_dict(), user_dict()],
        }
    }

    fn get(&self, name: &str) -> MachineResult<&Value> {
        self.stack
            .iter()
            .find_map(|dict| dict.get(name))
            .ok_or(MachineError::Undefined)
    }

    fn push(&mut self, dict: VariableDict) {
        self.stack.push(dict);
    }

    fn push_new(&mut self) {
        self.stack.push(VariableDict::new());
    }

    /// Pop the top dictionary from the stack. The first 3 dictionaries can not
    /// be popped, returns None if trying to pop them.
    fn pop(&mut self) -> Option<VariableDict> {
        (self.stack.len() > 3).then(|| self.stack.pop()).flatten()
    }
}

// dictionary operators

/// int dict -> dict
fn dict(m: &mut Machine) -> MachineResult<()> {
    let count = m.pop_int()?;
    m.push(Dictionary::with_capacity(count as usize));
    Ok(())
}

#[cfg(test)]
mod tests;
