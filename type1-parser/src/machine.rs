use crate::parser::{token as token_parser, white_space, white_space_or_comment, ws_prefixed};
use educe::Educe;
use either::Either;
use log::{debug, error};
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
    Name(Rc<str>),
    /// Tells eexec operation that works on current file.
    CurrentFile(
        #[educe(Debug(ignore))]
        #[educe(PartialEq(ignore))]
        Rc<RefCell<CurrentFile>>,
    ),
}

#[derive(Educe)]
#[educe(Debug, PartialEq, Clone)]
enum RuntimeValue {
    Value(Value),
    /// Mark stack position
    Mark,
    /// Tells ] operation that begin of array in stack.
    ArrayMark,
    Dictionary(Rc<RefCell<RuntimeDictionary>>),
    BuiltInOp(
        #[educe(Debug(ignore))]
        #[educe(PartialEq(ignore))]
        OperatorFn,
    ),
}

type RuntimeDictionary = HashMap<Key, RuntimeValue>;

macro_rules! value_access {
    ($method:ident, $opt_method:ident, $branch:ident, $t: ty) => {
        impl Value {
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
        }

        impl RuntimeValue {
            #[allow(dead_code)]
            pub fn $opt_method(&self) -> Option<$t> {
                match self {
                    Self::Value(Value::$branch(v)) => Some(v.clone()),
                    _ => None,
                }
            }

            #[allow(dead_code)]
            pub fn $method(&self) -> MachineResult<$t> {
                match self {
                    Self::Value(Value::$branch(v)) => Ok(v.clone()),
                    _ => Err(MachineError::TypeCheck),
                }
            }
        }
    };
}

macro_rules! rt_value_access {
    ($method:ident, $opt_method:ident, $branch:ident, $t: ty) => {
        impl RuntimeValue {
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
        }
    };
}

value_access!(bool, opt_bool, Bool, bool);
value_access!(int, opt_int, Integer, i32);
value_access!(real, opt_real, Real, f32);
value_access!(string, opt_string, String, Rc<RefCell<Vec<u8>>>);
value_access!(array, opt_array, Array, Rc<RefCell<Array>>);
rt_value_access!(dict, opt_dict, Dictionary, Rc<RefCell<RuntimeDictionary>>);
value_access!(procedure, opt_procedure, Procedure, Rc<TokenArray>);
value_access!(name, opt_name, Name, Rc<str>);
rt_value_access!(built_in_op, opt_built_in_op, BuiltInOp, OperatorFn);
value_access!(
    current_file,
    opt_current_file,
    CurrentFile,
    Rc<RefCell<CurrentFile>>
);

impl RuntimeValue {
    pub fn opt_number(&self) -> Option<Either<i32, f32>> {
        match self {
            Self::Value(Value::Integer(i)) => Some(Either::Left(*i)),
            Self::Value(Value::Real(r)) => Some(Either::Right(*r)),
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
    Name(Rc<str>),
}

impl TryFrom<RuntimeValue> for Key {
    type Error = MachineError;

    fn try_from(v: RuntimeValue) -> Result<Self, Self::Error> {
        match v {
            RuntimeValue::Value(Value::Bool(b)) => Ok(Self::Bool(b)),
            RuntimeValue::Value(Value::Integer(i)) => Ok(Self::Integer(i)),
            RuntimeValue::Value(Value::Name(n)) => Ok(Self::Name(n)),
            RuntimeValue::Value(Value::String(s)) => Ok(Self::Name(
                String::from_utf8(s.borrow().clone()).unwrap().into(),
            )),
            _ => Err(MachineError::TypeCheck),
        }
    }
}

impl From<TokenArray> for Value {
    fn from(v: TokenArray) -> Self {
        Self::Procedure(v.into())
    }
}

impl From<Value> for RuntimeValue {
    fn from(v: Value) -> Self {
        Self::Value(v)
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
            Key::Name(n) => n,
        }
    }
}

pub type Dictionary = HashMap<Key, Value>;

#[derive(Debug, PartialEq, Clone)]
pub enum Token {
    Literal(Value),
    /// Name to lookup operation dict to get the actual operator
    Name(Rc<str>),
}

#[cfg(test)]
pub fn name_token(s: impl Into<Rc<str>>) -> Token {
    Token::Name(s.into())
}

impl TryFrom<RuntimeValue> for Value {
    type Error = MachineError;

    fn try_from(v: RuntimeValue) -> Result<Self, Self::Error> {
        match v {
            RuntimeValue::Value(v) => Ok(v),
            _ => Err(MachineError::TypeCheck),
        }
    }
}

macro_rules! to_value {
    ($t:ty, $branch:ident) => {
        impl From<$t> for Value {
            fn from(v: $t) -> Self {
                Self::$branch(v)
            }
        }

        impl From<$t> for RuntimeValue {
            fn from(v: $t) -> Self {
                Self::Value(Value::$branch(v))
            }
        }
    };
}
to_value!(bool, Bool);
to_value!(i32, Integer);
to_value!(f32, Real);
to_value!(Rc<RefCell<Vec<u8>>>, String);

impl<const N: usize> From<[u8; N]> for Value {
    fn from(v: [u8; N]) -> Self {
        let bytes: Vec<u8> = v.into();
        bytes.into()
    }
}

impl<const N: usize> From<[u8; N]> for RuntimeValue {
    fn from(v: [u8; N]) -> Self {
        let bytes: Vec<u8> = v.into();
        bytes.into()
    }
}

impl From<Vec<u8>> for RuntimeValue {
    fn from(v: Vec<u8>) -> Self {
        Value::String(Rc::new(RefCell::new(v))).into()
    }
}

impl From<Vec<u8>> for Value {
    fn from(v: Vec<u8>) -> Self {
        Self::String(Rc::new(RefCell::new(v)))
    }
}

impl From<&str> for RuntimeValue {
    fn from(v: &str) -> Self {
        Value::Name(v.to_owned().into()).into()
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Self::Name(v.to_owned().into())
    }
}

impl From<Array> for Value {
    fn from(v: Array) -> Self {
        Self::Array(Rc::new(RefCell::new(v)))
    }
}

impl From<Array> for RuntimeValue {
    fn from(v: Array) -> Self {
        Value::Array(Rc::new(RefCell::new(v))).into()
    }
}

impl From<RuntimeDictionary> for RuntimeValue {
    fn from(v: RuntimeDictionary) -> Self {
        RuntimeValue::Dictionary(Rc::new(RefCell::new(v)))
    }
}

impl From<Rc<RefCell<RuntimeDictionary>>> for RuntimeValue {
    fn from(v: Rc<RefCell<RuntimeDictionary>>) -> Self {
        Self::Dictionary(v)
    }
}

fn into_dict(d: RuntimeDictionary) -> MachineResult<Dictionary> {
    let mut dict = Dictionary::new();
    for (k, v) in d {
        let v = match v {
            RuntimeValue::Value(v) => v,
            RuntimeValue::Dictionary(d) => {
                let d: RuntimeDictionary =
                    Rc::try_unwrap(d).map_or_else(|d| d.borrow().clone(), |d| d.into_inner());
                Value::Dictionary(Rc::new(RefCell::new(into_dict(d)?)))
            }
            _ => return Err(MachineError::TypeCheck),
        };
        dict.insert(k, v);
    }
    Ok(dict)
}

impl<T: Into<Value>> From<T> for Token {
    fn from(v: T) -> Self {
        Token::Literal(v.into())
    }
}

/// Create Array from a list of values that implement Into<Object> trait
#[cfg(test)]
macro_rules! values {
    () => {
        Array::new()
    };
    ($($e:expr),*) => {
        vec![$(Into::<Value>::into($e)),*]
    }
}

#[cfg(test)]
macro_rules! rt_values {
    () => {
        Array::new()
    };
    ($($e:expr),*) => {
        vec![$(Into::<RuntimeValue>::into($e)),*]
    }
}

#[derive(Debug, PartialEq, thiserror::Error)]
pub enum MachineError {
    #[error("stack underflow")]
    StackUnderflow,
    #[error("type check error")]
    TypeCheck,
    #[error("undefined")]
    Undefined,
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
        self.skip_white_space();
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

        let remains = &self.data[self.remains_pos..];
        repeat::<_, _, (), _, _>(.., white_space_or_comment)
            .parse(remains)
            .unwrap();
        self.remains_pos = self.data.len() - remains.len();
    }
}

/// PostScript machine to execute operations.
pub struct Machine {
    file: Rc<RefCell<CurrentFile>>,
    variable_stack: VariableDictStack,
    stack: Vec<RuntimeValue>,
    fonts: Vec<(String, Dictionary)>,
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
            fonts: vec![],
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

    pub fn take_fonts(self) -> Vec<(String, Dictionary)> {
        self.fonts
    }

    fn exec(&mut self, token: Token) -> MachineResult<ExecState> {
        Ok(match token {
            Token::Literal(v) => {
                self.push(v);
                ExecState::Ok
            }
            Token::Name(name) => {
                debug!("{}", name);
                let v = self.variable_stack.get(&name)?;
                match v {
                    RuntimeValue::BuiltInOp(op) => op(self)?,
                    RuntimeValue::Value(Value::Procedure(p)) => self.execute_procedure(p)?,
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

    fn dump_stack(&self) {
        debug!(
            "stack: {:?}",
            self.stack
                .iter()
                .rev()
                .map(std::mem::discriminant)
                .collect::<Vec<_>>()
        );
    }

    fn pop(&mut self) -> MachineResult<RuntimeValue> {
        let r = self.stack.pop().ok_or(MachineError::StackUnderflow);
        self.dump_stack();
        r
    }

    fn top(&self) -> MachineResult<&RuntimeValue> {
        self.stack.last().ok_or(MachineError::StackUnderflow)
    }

    fn pop_int(&mut self) -> MachineResult<i32> {
        self.pop().and_then(|v| v.int())
    }

    fn pop_dict(&mut self) -> MachineResult<Rc<RefCell<RuntimeDictionary>>> {
        self.pop().and_then(|v| v.dict())
    }

    fn push(&mut self, v: impl Into<RuntimeValue>) {
        self.stack.push(v.into());
        self.dump_stack();
    }

    fn push_current_file(&mut self) {
        self.push(Value::CurrentFile(self.file.clone()))
    }

    fn define_font(&mut self, name: String, font: Dictionary) {
        self.fonts.push((name, font));
    }
}

struct VariableDictStack {
    stack: Vec<Rc<RefCell<RuntimeDictionary>>>,
}

macro_rules! var_dict {
    ($($k:expr => $v:expr),* $(,)?) => {
        std::iter::Iterator::collect(std::iter::IntoIterator::into_iter([$((Key::Name($k.to_owned().into()), RuntimeValue::BuiltInOp($v)),)*]))
    };
}

#[cfg(test)]
macro_rules! dict {
    () => {
        Dictionary::new()
    };
    ($($k:expr => $v:expr),* $(,)?) => {
        std::iter::Iterator::collect::<RuntimeDictionary>(std::iter::IntoIterator::into_iter([$((Key::Name($k.to_owned().into()), RuntimeValue::from($v)),)*]))
    };
}

/// Create the `systemdict`
fn system_dict() -> RuntimeDictionary {
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
            m.push(RuntimeValue::Mark);
            ok()
        },
        // Mark obj1 .. obj(n) cleartomark -> -
        "cleartomark" => |m| {
            while m.pop()
                .map_err(|e| if e == MachineError::StackUnderflow {MachineError::UnMatchedMark } else {e})?
                 != RuntimeValue::Mark {}
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
            m.push(RuntimeValue::ArrayMark);
            ok()
        },
        "]" => |m| {
            let mut array = Array::new();
            loop {
                match m.pop()? {
                    RuntimeValue::ArrayMark => break,
                    RuntimeValue::Value(v) => array.push(v),
                    _ => return Err(MachineError::TypeCheck),
                }
            }
            array.reverse();
            m.push(array);
            ok()
        },
        "[]" => |m| {
            m.push(Array::new());
            ok()
        },

        // int dict -> dict
        "dict" => |m| {
            let count = m.pop_int()?;
            m.push(RuntimeDictionary::with_capacity(count as usize));
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
            match m.pop()?{
                RuntimeValue::Dictionary(dict) => {
                    dict.borrow_mut().insert(key.try_into()?, value);
                }
                RuntimeValue::Value(Value::Array(array)) => {
                    let index = key.int()?;
                    let mut array = array.borrow_mut();
                    array.resize(index as usize + 1, Value::Null);
                    array[index as usize] = value.try_into()?;
                }
                v => {
                    error!("put on non-dict/array: {:?}, key: {:?}, value: {:?}", v, key, value);
                    return Err(MachineError::TypeCheck);
                }
            };
            ok()
        },
        "get" => |m| {
            let key = m.pop()?;
            let dict = m.pop_dict()?;
            let key: Key = key.try_into()?;
            let value = dict.borrow().get(&key).cloned().ok_or(MachineError::Undefined)?;
            m.push(value);
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
            m.push(m.variable_stack.top());
            ok()
        },
        "currentfile" => |m| {
            m.push_current_file();
            ok()
        },
        "readstring" => |m| {
            let s = m.pop()?.string()?;
            let f = m.pop()?.current_file()?;
            let mut borrow = s.borrow_mut();
            let buf = &mut borrow[..];
            let eof = f.borrow_mut().read(buf) < buf.len();
            drop(borrow);
            m.push(s);
            m.push(!eof);
            ok()
        },

        // initial increment limit proc for -
        "for" => |m| {
            let proc = m.pop()?.procedure()?;
            let limit = m.pop_int()?;
            let increment = m.pop_int()?;
            let initial = m.pop_int()?;
            for i in (initial..=limit).step_by(increment as usize) {
                m.push(i);
                m.execute_procedure(proc.clone())?;
            }
            ok()
        },
        "eexec" => |m| {
            assert!(
                matches!(m.pop()?, RuntimeValue::Value(Value::CurrentFile(_))),
                "eexec on non-current file not implemented"
            );
            m.variable_stack.push_system_dict();
            Ok(ExecState::StartEExec)
        },
        // file closefile -
        "closefile" => |m| {
            let RuntimeValue::Value(Value::CurrentFile(_f)) = m.pop()? else {
                return Err(MachineError::TypeCheck);
            };
            Ok(ExecState::EndEExec)
        },
        "definefont" => |m| {
            let font = m.pop()?;
            let key = m.pop()?;
            let name = key.name()?;
            let name = (*name).to_owned();
            m.define_font(name, into_dict(font.dict()?.borrow().clone())?);
            m.push(font);
            ok()
        },

        "readonly" => |_| ok(),
        "executeonly" => |_| ok(),
        "noaccess" => |_| ok(),
    )
}

/// Create the `globaldict`
fn global_dict() -> RuntimeDictionary {
    RuntimeDictionary::new()
}

/// Create the `userdict`
fn user_dict() -> RuntimeDictionary {
    RuntimeDictionary::new()
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

    fn get(&self, name: &str) -> MachineResult<RuntimeValue> {
        let r = self
            .stack
            .iter()
            .find_map(|dict| dict.borrow().get(name).cloned())
            .ok_or(MachineError::Undefined);
        #[cfg(debug_assertions)]
        if r.is_err() {
            error!("name not found: {:?}", name);
        }
        r
    }

    fn push(&mut self, dict: Rc<RefCell<RuntimeDictionary>>) {
        self.stack.push(dict);
    }

    /// Pop the top dictionary from the stack. The first 3 dictionaries can not
    /// be popped, returns None if trying to pop them.
    fn pop(&mut self) -> Option<Rc<RefCell<RuntimeDictionary>>> {
        (self.stack.len() > 3).then(|| self.stack.pop()).flatten()
    }

    fn top(&self) -> Rc<RefCell<RuntimeDictionary>> {
        self.stack.last().unwrap().clone()
    }

    fn lock_system_dict(&self) -> Ref<RuntimeDictionary> {
        self.stack[0].borrow()
    }
}

#[cfg(test)]
mod tests;
