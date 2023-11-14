use educe::Educe;
use std::{cell::RefCell, collections::HashMap, hash::Hasher, iter::repeat, rc::Rc};

pub type Array = Vec<Value>;
pub type TokenArray = Vec<Token>;

type OperatorFn = fn(&mut Machine) -> MachineResult<()>;

#[derive(Educe)]
#[educe(Debug, PartialEq, Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Integer(i32),
    Real(f32),
    String(Rc<[u8]>),
    Array(Rc<RefCell<Array>>),
    Dictionary(Rc<RefCell<Dictionary>>),
    Procedure(Rc<TokenArray>),
    Name(Rc<String>),
    BuiltInOp(
        #[educe(Debug(ignore))]
        #[educe(PartialEq(ignore))]
        OperatorFn,
    ),
}

macro_rules! value_access {
    ($method:ident, $opt_method:ident, $branch:ident, $t: ty) => {
        #[allow(dead_code)]
        pub fn $opt_method(&self) -> Option<$t> {
            match self {
                Self::$branch(v) => Some(v.clone()),
                _ => None,
            }
        }

        #[allow(dead_code)]
        pub fn $method(&self) -> MachineResult<$t> {
            match self {
                Self::$branch(v) => Ok(v.clone()),
                _ => Err(MachineError::TypeCheck),
            }
        }
    };
}

impl Value {
    value_access!(bool, opt_bool, Bool, bool);
    value_access!(int, opt_int, Integer, i32);
    value_access!(real, opt_real, Real, f32);
    value_access!(string, opt_string, String, Rc<[u8]>);
    value_access!(array, opt_array, Array, Rc<RefCell<Array>>);
    value_access!(dict, opt_dict, Dictionary, Rc<RefCell<Dictionary>>);
    value_access!(procedure, opt_procedure, Procedure, Rc<TokenArray>);
    value_access!(name, opt_name, Name, Rc<String>);
    value_access!(built_in_op, opt_built_in_op, BuiltInOp, OperatorFn);
}

/// Type of `Dictionary` key. PostScript allows any value to be key except null,
/// String will convert to Name when used as key.
/// But I don't want to implement that, so I will only allow `Bool`, `Integer`,
/// and `Name`, and convert `String` to `Name` when used as key.
/// We'll figure it out if encounter other types, it should not happen in practice.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Key {
    Bool(bool),
    Integer(i32),
    Name(Rc<String>),
}

impl TryFrom<Value> for Key {
    type Error = MachineError;

    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::Bool(b) => Ok(Self::Bool(b)),
            Value::Integer(i) => Ok(Self::Integer(i)),
            Value::Name(n) => Ok(Self::Name(n)),
            Value::String(s) => Ok(Self::Name(String::from_utf8(s.to_vec()).unwrap().into())),
            _ => Err(MachineError::TypeCheck),
        }
    }
}

/// Custom Hash to allow &str to lookup Dictionary,
/// Key implement Borrow<str> trait, hash(str) should equal to hash(Key::Name)
impl std::hash::Hash for Key {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::Bool(b) => b.hash(state),
            Self::Integer(i) => i.hash(state),
            Self::Name(n) => n.hash(state),
        }
    }
}

impl std::borrow::Borrow<str> for Key {
    fn borrow(&self) -> &str {
        match self {
            // return a string that will never be a valid name to never select bool key using str
            Key::Bool(_) => "$$bool$$",
            Key::Integer(_) => "$$int$$",
            Key::Name(n) => n.as_str(),
        }
    }
}

pub type Dictionary = HashMap<Key, Value>;

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
        Value::Name(v.to_owned().into())
    }
}

impl From<Array> for Value {
    fn from(v: Array) -> Self {
        Value::Array(Rc::new(RefCell::new(v)))
    }
}

impl From<TokenArray> for Value {
    fn from(v: TokenArray) -> Self {
        Value::Procedure(v.into())
    }
}

impl From<Dictionary> for Value {
    fn from(v: Dictionary) -> Self {
        Value::Dictionary(Rc::new(RefCell::new(v)))
    }
}

impl From<Rc<RefCell<Dictionary>>> for Value {
    fn from(v: Rc<RefCell<Dictionary>>) -> Self {
        Value::Dictionary(v)
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
macro_rules! values {
    () => {
        Array::new()
    };
    ($($e:expr),*) => {
        vec![$(Into::<Value>::into($e)),*]
    }
}
pub(crate) use values;

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

    fn top(&self) -> MachineResult<&Value> {
        self.stack.last().ok_or(MachineError::StackUnderflow)
    }

    fn pop_int(&mut self) -> MachineResult<i32> {
        self.pop().and_then(|v| match v {
            Value::Integer(i) => Ok(i),
            _ => Err(MachineError::TypeCheck),
        })
    }

    fn pop_dict(&mut self) -> MachineResult<Rc<RefCell<Dictionary>>> {
        self.pop().and_then(|v| match v {
            Value::Dictionary(d) => Ok(d),
            _ => Err(MachineError::TypeCheck),
        })
    }

    fn push(&mut self, v: impl Into<Value>) {
        self.stack.push(v.into());
    }
}

struct VariableDictStack {
    stack: Vec<Rc<RefCell<Dictionary>>>,
}

macro_rules! var_dict {
    ($($k:expr => $v:expr),* $(,)?) => {
        std::iter::Iterator::collect(std::iter::IntoIterator::into_iter([$((Key::Name($k.to_owned().into()), Value::BuiltInOp($v)),)*]))
    };
}

macro_rules! dict {
    () => {
        Dictionary::new()
    };
    ($($k:expr => $v:expr),* $(,)?) => {
        std::iter::Iterator::collect::<Dictionary>(std::iter::IntoIterator::into_iter([$((Key::Name($k.to_owned().into()), Value::from($v)),)*]))
    };
}

/// Create the `systemdict`
fn system_dict() -> Dictionary {
    var_dict!(
        // any1 any2 exch -> any2 any1
        "exch" => |m| {
            let a = m.pop()?;
            let b = m.pop()?;
            m.push(a);
            m.push(b);
            Ok(())
        },

        // any -> any any
        "dup" => |m| Ok(m.push(m.top()?.clone())),

        // Duplicate stack value at -n position
        // anyn ... any0 n index -> anyn ... any0 anyn
        "index" => |m| {
            let index = m.pop_int()?;
            Ok(m.push(m.stack.get(m.stack.len() - index as usize - 1)
                .ok_or(MachineError::StackUnderflow)?
                .clone()))
        },

        // int array -> array
        "array" => |m| {
            let count = m.pop_int()?;
            Ok(m.push(Array::from_iter(repeat(Value::Null).take(count as usize))))
        },

        // int dict -> dict
        "dict" => |m| {
            let count = m.pop_int()?;
            Ok(m.push(Dictionary::with_capacity(count as usize)))
        },

        // dict begin -> -
        "begin" => |m| {
            let dict = m.pop_dict()?;
            m.variable_stack.push(dict);
            Ok(())
        },

        // - end -> -
        "end" => |m| {
            m.variable_stack.pop();
            Ok(())
        },

        // key value -> - Set key-value to current directory.
        "def" => |m| {
            let value = m.pop()?;
            let key = m.pop()?;
            let dict = m.variable_stack.top();
            dict.borrow_mut().insert(key.try_into()?, value);
            Ok(())
        },

        // dict key value put -
        // Set key-value to the given dictionary.
        "put" => |m| {
            let value = m.pop()?;
            let key = m.pop()?;
            let dict = m.pop_dict()?;
            dict.borrow_mut().insert(key.try_into()?, value);
            Ok(())
        },

        // push current variable stack to operand stack
        "currentdict" => |m| {
            let dict = m.variable_stack.top();
            m.push(dict.clone());
            Ok(())
        },

        "readonly" => |_| Ok(()),
    )
}

/// Create the `globaldict`
fn global_dict() -> Dictionary {
    let mut dict = Dictionary::new();
    // dict.insert("globaldict".to_owned(), Value::Dictionary(Rc::new(dict)));
    dict
}

/// Create the `userdict`
fn user_dict() -> Dictionary {
    let mut dict = Dictionary::new();
    // dict.insert("userdict".to_owned(), Value::Dictionary(Rc::new(dict)));
    dict
}

impl VariableDictStack {
    fn new() -> Self {
        Self {
            stack: vec![
                Rc::new(RefCell::new(system_dict())),
                Rc::new(RefCell::new(global_dict())),
                Rc::new(RefCell::new(user_dict())),
            ],
        }
    }

    fn get(&self, name: &str) -> MachineResult<Value> {
        self.stack
            .iter()
            .find_map(|dict| dict.borrow().get(name).map(|v| v.clone()))
            .ok_or(MachineError::Undefined)
    }

    fn push(&mut self, dict: Rc<RefCell<Dictionary>>) {
        self.stack.push(dict);
    }

    /// Pop the top dictionary from the stack. The first 3 dictionaries can not
    /// be popped, returns None if trying to pop them.
    fn pop(&mut self) -> Option<Rc<RefCell<Dictionary>>> {
        (self.stack.len() > 3).then(|| self.stack.pop()).flatten()
    }

    fn top(&self) -> Rc<RefCell<Dictionary>> {
        self.stack.last().unwrap().clone()
    }
}

#[cfg(test)]
mod tests;
