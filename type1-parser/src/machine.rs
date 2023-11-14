use crate::parser::{token as token_parser, white_space, white_space_or_comment, ws_prefixed};
use educe::Educe;
use either::Either;
use log::error;
use std::{
    cell::{Ref, RefCell},
    collections::HashMap,
    hash::Hasher,
    iter::repeat,
    rc::Rc,
};
use winnow::Parser;

mod decrypt;
use decrypt::{decrypt, EEXEC_KEY};

pub type Array = Vec<Value>;
pub type TokenArray = Vec<Token>;

type OperatorFn = fn(&mut Machine) -> MachineResult<ExecState>;

#[derive(Educe)]
#[educe(Debug, PartialEq, Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Integer(i32),
    Real(f32),
    String(Rc<RefCell<Vec<u8>>>),
    Array(Rc<RefCell<Array>>),
    Dictionary(Rc<RefCell<Dictionary>>),
    Procedure(Rc<TokenArray>),
    Name(Rc<String>),
    BuiltInOp(
        #[educe(Debug(ignore))]
        #[educe(PartialEq(ignore))]
        OperatorFn,
    ),
    /// Mark stack position
    Mark,
    /// Tells eexec operation that works on current file.
    CurrentFile(
        #[educe(Debug(ignore))]
        #[educe(PartialEq(ignore))]
        Rc<RefCell<CurrentFile>>,
    ),
    /// Tells ] operation that begin of array in stack.
    ArrayMark,
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
    value_access!(string, opt_string, String, Rc<RefCell<Vec<u8>>>);
    value_access!(array, opt_array, Array, Rc<RefCell<Array>>);
    value_access!(dict, opt_dict, Dictionary, Rc<RefCell<Dictionary>>);
    value_access!(procedure, opt_procedure, Procedure, Rc<TokenArray>);
    value_access!(name, opt_name, Name, Rc<String>);
    value_access!(built_in_op, opt_built_in_op, BuiltInOp, OperatorFn);
    value_access!(
        current_file,
        opt_current_file,
        CurrentFile,
        Rc<RefCell<CurrentFile>>
    );

    pub fn opt_number(&self) -> Option<Either<i32, f32>> {
        match self {
            Self::Integer(i) => Some(Either::Left(*i)),
            Self::Real(r) => Some(Either::Right(*r)),
            _ => None,
        }
    }

    pub fn number(&self) -> MachineResult<Either<i32, f32>> {
        self.opt_number().ok_or(MachineError::TypeCheck)
    }
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
            Value::String(s) => Ok(Self::Name(
                String::from_utf8(s.borrow().clone()).unwrap().into(),
            )),
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

#[derive(Debug, PartialEq, Clone)]
pub enum Token {
    Literal(Value),
    /// Name to lookup operation dict to get the actual operator
    Name(Rc<String>),
}

pub fn name_token(s: impl Into<String>) -> Token {
    Token::Name(Rc::new(s.into()))
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
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
        let bytes: Vec<u8> = v.into();
        bytes.into()
    }
}

impl From<Vec<u8>> for Value {
    fn from(v: Vec<u8>) -> Self {
        Value::String(Rc::new(RefCell::new(v.into())))
    }
}

impl From<Rc<RefCell<Vec<u8>>>> for Value {
    fn from(v: Rc<RefCell<Vec<u8>>>) -> Self {
        Value::String(v)
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
    #[error("unmatched mark")]
    UnMatchedMark,
}

pub type MachineResult<T> = Result<T, MachineError>;

pub struct CurrentFile {
    data: Vec<u8>,
    remains_pos: usize,
    decryped: Option<Vec<u8>>,
    decryped_pos: usize,
}

impl CurrentFile {
    pub fn new(data: Vec<u8>) -> Self {
        Self {
            data,
            remains_pos: 0,
            decryped: None,
            decryped_pos: 0,
        }
    }

    pub fn next_token(&mut self) -> Option<Token> {
        match self.decryped {
            Some(ref data) => {
                let mut buf = &data[self.decryped_pos..];
                let r = ws_prefixed(token_parser).parse_next(&mut buf).ok();
                self.decryped_pos = data.len() - buf.len();
                r
            }
            None => {
                let mut remains = &self.data[self.remains_pos..];
                let r = ws_prefixed(token_parser).parse_next(&mut remains).ok();
                self.remains_pos = self.data.len() - remains.len();
                r
            }
        }
    }

    pub fn skip_white_space(&mut self) {
        match self.decryped {
            Some(ref data) => {
                let mut buf = &data[self.decryped_pos..];
                white_space.parse_next(&mut buf).unwrap();
                self.decryped_pos = data.len() - buf.len();
            }
            None => {
                let mut remains = &self.data[self.remains_pos..];
                white_space.parse_next(&mut remains).unwrap();
                self.remains_pos = self.data.len() - remains.len();
            }
        }
    }

    pub fn start_decrypt(&mut self) {
        assert!(self.decryped.is_none());
        self.skip_white_space();
        let remains = &self.data[self.remains_pos..];
        self.decryped = Some(decrypt(EEXEC_KEY, 4, remains));
        self.remains_pos += 4;
        self.decryped_pos = 0;
    }

    pub fn stop_decrypt(&mut self) {
        assert!(self.decryped.is_some());
        self.remains_pos += self.decryped_pos;
        self.decryped = None;
    }

    pub fn read(&mut self, buf: &mut [u8]) -> usize {
        self.skip_white_space();
        match self.decryped {
            Some(ref data) => {
                let len = buf.len().min(data.len() - self.decryped_pos);
                buf[..len].copy_from_slice(&data[self.decryped_pos..(self.decryped_pos + len)]);
                self.decryped_pos += len;
                len
            }
            None => {
                let len = buf.len().min(self.data.len() - self.remains_pos);
                buf[..len].copy_from_slice(&self.data[self.remains_pos..(self.remains_pos + len)]);
                self.remains_pos += len;
                len
            }
        }
    }

    /// Check file read complete
    pub fn finish(&mut self) {
        use winnow::combinator::repeat;

        let mut remains = &self.data[self.remains_pos..];
        repeat::<_, _, (), _, _>(.., white_space_or_comment)
            .parse(&mut remains)
            .unwrap();
        self.remains_pos = self.data.len() - remains.len();
    }
}

/// PostScript machine to execute operations.
pub struct Machine {
    file: Rc<RefCell<CurrentFile>>,
    variable_stack: VariableDictStack,
    stack: Vec<Value>,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ExecState {
    Ok,
    // starts decrypt if exec() returns this
    StartEExec,
    // ends decrypt if exec() returns this
    EndEExec,
}

impl Machine {
    pub fn new(file: Vec<u8>) -> Self {
        Self {
            file: Rc::new(RefCell::new(CurrentFile::new(file))),
            variable_stack: VariableDictStack::new(),
            stack: Vec::new(),
        }
    }

    pub fn execute(&mut self) -> MachineResult<()> {
        // ensure that the system_dict readonly, it will panic if modify
        // system_dict
        self.variable_stack.lock_system_dict();

        while let Some(token) = {
            let mut b = self.file.borrow_mut();
            b.next_token()
        } {
            match self.exec(token)? {
                ExecState::Ok => {}
                ExecState::StartEExec => {
                    self.file.borrow_mut().start_decrypt();
                }
                ExecState::EndEExec => {
                    self.file.borrow_mut().stop_decrypt();
                }
            }
        }
        // assert that remains are all white space or comment
        self.file.borrow_mut().finish();

        Ok(())
    }

    fn exec(&mut self, token: Token) -> MachineResult<ExecState> {
        Ok(match token {
            Token::Literal(v) => {
                self.push(v);
                ExecState::Ok
            }
            Token::Name(name) => {
                let v = self.variable_stack.get(&name)?;
                match v {
                    Value::BuiltInOp(op) => op(self)?,
                    Value::Procedure(p) => self.execute_procedure(p)?,
                    v => unreachable!("{:?}", v),
                }
            }
        })
    }

    fn execute_procedure(&mut self, proc: Rc<TokenArray>) -> MachineResult<ExecState> {
        for token in proc.as_ref().iter().cloned() {
            assert_eq!(
                self.exec(token)?,
                ExecState::Ok,
                "procedure should not return StartEExec or EndEExec"
            );
        }
        Ok(ExecState::Ok)
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

    fn push_current_file(&mut self) {
        self.stack.push(Value::CurrentFile(self.file.clone()))
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
    fn ok() -> MachineResult<ExecState> {
        Ok(ExecState::Ok)
    }

    var_dict!(
        // any1 any2 exch -> any2 any1
        "exch" => |m| {
            let a = m.pop()?;
            let b = m.pop()?;
            m.push(a);
            m.push(b);
            ok()
        },

        // any -> any any
        "dup" => |m| {
            m.push(m.top()?.clone());
            ok()
        },
        // any pop -
        "pop" => |m| {
            m.pop()?;
            ok()
        },

        // Duplicate stack value at -n position
        // any(n) ... any0 n index -> any(n) ... any0 any(n)
        "index" => |m| {
            let index = m.pop_int()?;
            m.push(m.stack.get(m.stack.len() - index as usize - 1)
                .ok_or(MachineError::StackUnderflow)?
                .clone());
            ok()
        },

        // - mark -> Mark
        "mark" => |m| {
            m.push(Value::Mark);
            ok()
        },
        // Mark obj1 .. obj(n) cleartomark -> -
        "cleartomark" => |m| {
            while m.pop()
                .map_err(|e| if e == MachineError::StackUnderflow {MachineError::UnMatchedMark } else {e})?
                 != Value::Mark {}
            ok()
        },

        // - true -> true
        "true" => |m| {
            m.push(true);
            ok()
        },
        // - false -> false
        "false" => |m| {
            m.push(false);
            ok()
        },

        // num1 num2 add sum
        "add" => |m| {
            let a = m.pop()?.number()?;
            let b = m.pop()?.number()?;
            match (a, b) {
                (Either::Left(a), Either::Left(b)) => m.push(a + b),
                (Either::Right(a), Either::Right(b)) => m.push(a + b),
                (Either::Left(a), Either::Right(b)) => m.push(a as f32 + b),
                (Either::Right(a), Either::Left(b)) => m.push(a + b as f32),
            }
            ok()
        },

        // int array -> array
        "array" => |m| {
            let count = m.pop_int()?;
            m.push(Array::from_iter(repeat(Value::Null).take(count as usize)));
            ok()
        },
        "[" => |m| {
            m.push(Value::ArrayMark);
            ok()
        },
        "]" => |m| {
            let mut array = Array::new();
            loop {
                match m.pop()? {
                    Value::ArrayMark => break,
                    v => array.push(v),
                }
            }
            array.reverse();
            m.push(array);
            ok()
        },

        // int dict -> dict
        "dict" => |m| {
            let count = m.pop_int()?;
            m.push(Dictionary::with_capacity(count as usize));
            ok()
        },

        // dict begin -> -
        "begin" => |m| {
            let dict = m.pop_dict()?;
            m.variable_stack.push(dict);
            ok()
        },

        // - end -> -
        "end" => |m| {
            m.variable_stack.pop();
            ok()
        },

        // key value -> - Set key-value to current directory.
        "def" => |m| {
            let value = m.pop()?;
            let key = m.pop()?;
            let dict = m.variable_stack.top();
            dict.borrow_mut().insert(key.try_into()?, value);
            ok()
        },

        // dict/array key value put -
        // Set key-value to the given dictionary.
        "put" => |m| {
            let value = m.pop()?;
            let key = m.pop()?;
            match m.pop()? {
                Value::Dictionary(dict) => {
                    dict.borrow_mut().insert(key.try_into()?, value);
                }
                Value::Array(array) => {
                    let index = key.int()?;
                    let mut array = array.borrow_mut();
                    array.resize(index as usize + 1, Value::Null);
                    array[index as usize] = value;
                }
                _ => return Err(MachineError::TypeCheck),
            };
            ok()
        },

        // int string -> string
        "string" => |m| {
            let count = m.pop_int()?;
            m.push(vec![0u8; count as usize]);
            ok()
        },

        // push current variable stack to operand stack
        "currentdict" => |m| {
            let dict = m.variable_stack.top();
            m.push(dict.clone());
            ok()
        },
        "currentfile" => |m| {
            m.push_current_file();
            ok()
        },
        "readstring" => |m| {
            let s = m.pop()?.string()?;
            let f = m.pop()?.current_file()?;
            let mut buf = &mut s.borrow_mut()[..];
            let eof = f.borrow_mut().read(&mut buf) < buf.len();
            m.push(s.clone());
            m.push(eof);
            ok()
        },

        // initial increment limit proc for -
        "for" => |m| {
            let proc = m.pop()?.procedure()?;
            let limit = m.pop_int()?;
            let increment = m.pop_int()?;
            let initial = m.pop_int()?;
            for i in (initial..=limit).into_iter().step_by(increment as usize) {
                m.push(i);
                m.execute_procedure(proc.clone())?;
            }
            ok()
        },
        "eexec" => |m| {
            assert!(
                matches!(m.top()?, Value::CurrentFile(_)),
                "eexec on non-current file not implemented"
            );
            m.variable_stack.push_system_dict();
            Ok(ExecState::StartEExec)
        },
        // file closefile -
        "closefile" => |m| {
            let Value::CurrentFile(_) = m.pop()? else {
                return Err(MachineError::TypeCheck);
            };
            Ok(ExecState::EndEExec)
        },

        "readonly" => |_| ok(),
        "executeonly" => |_| ok(),
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

    fn push_system_dict(&mut self) {
        self.stack.push(self.stack[0].clone());
    }

    fn get(&self, name: &str) -> MachineResult<Value> {
        let r = self
            .stack
            .iter()
            .find_map(|dict| dict.borrow().get(name).map(|v| v.clone()))
            .ok_or(MachineError::Undefined);
        #[cfg(debug_assertions)]
        if r.is_err() {
            error!("name not found: {:?}", name);
        }
        r
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

    fn lock_system_dict(&self) -> Ref<Dictionary> {
        self.stack[0].borrow()
    }
}

#[cfg(test)]
mod tests;
