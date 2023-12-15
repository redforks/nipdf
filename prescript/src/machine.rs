use crate::{
    name,
    parser::{token as token_parser, white_space, white_space_or_comment, ws_prefixed},
    sname, Name,
};
use educe::Educe;
use either::Either;
use std::{
    cell::{Ref, RefCell},
    collections::HashMap,
    fmt::Display,
    hash::Hasher,
    iter::repeat,
    rc::Rc,
    str::from_utf8,
};
use winnow::Parser;

mod decrypt;
use decrypt::{decrypt, EEXEC_KEY};
use log::error;
mod cidinit;

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
    Dictionary(Dictionary),
    Procedure(Rc<RefCell<TokenArray>>),
    Name(Name),
    PredefinedEncoding(Name),
}

#[derive(Educe)]
#[educe(Debug, PartialEq, Clone)]
enum RuntimeValue<'a> {
    Value(Value),
    /// Mark stack position
    Mark,
    /// Tells ] operation that begin of array in stack.
    ArrayMark,
    /// Tell >> operation that end of dictionary in stack.
    DictMark,
    Dictionary(Rc<RefCell<RuntimeDictionary<'a>>>),
    BuiltInOp(
        #[educe(Debug(ignore))]
        #[educe(PartialEq(ignore))]
        OperatorFn,
    ),
    /// Tells eexec operation that works on current file.
    CurrentFile(
        #[educe(Debug(ignore))]
        #[educe(PartialEq(ignore))]
        Rc<RefCell<CurrentFile<'a>>>,
    ),
}

impl<'b> Display for RuntimeValue<'b> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeValue::Mark => write!(f, "mark"),
            RuntimeValue::ArrayMark => write!(f, "array-mark"),
            RuntimeValue::DictMark => write!(f, "dict-mark"),
            RuntimeValue::Dictionary(_) => write!(f, "dict"),
            RuntimeValue::BuiltInOp(_) => write!(f, "built-in-op"),
            RuntimeValue::CurrentFile(_) => write!(f, "current-file"),
            RuntimeValue::Value(v) => match v {
                Value::Null => write!(f, "null"),
                Value::Bool(b) => {
                    if *b {
                        write!(f, "true")
                    } else {
                        write!(f, "false")
                    }
                }
                Value::Integer(i) => write!(f, "{}", i),
                Value::Real(r) => write!(f, "{}", r),
                Value::String(_) => write!(f, "string"),
                Value::Array(_) => write!(f, "array"),
                Value::Dictionary(_) => write!(f, "dict"),
                Value::Procedure(_) => write!(f, "procedure"),
                Value::Name(n) => write!(f, "/{}", n),
                Value::PredefinedEncoding(_) => write!(f, "encoding"),
            },
        }
    }
}

impl<'a> From<Name> for RuntimeValue<'a> {
    fn from(v: Name) -> Self {
        Self::Value(Value::Name(v))
    }
}

type RuntimeDictionary<'a> = HashMap<Key, RuntimeValue<'a>>;

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

        impl<'a> RuntimeValue<'a> {
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
        impl<'a> RuntimeValue<'a> {
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
rt_value_access!(
    dict,
    opt_dict,
    Dictionary,
    Rc<RefCell<RuntimeDictionary<'a>>>
);
value_access!(procedure, opt_procedure, Procedure, Rc<RefCell<TokenArray>>);
value_access!(name, opt_name, Name, Name);
rt_value_access!(built_in_op, opt_built_in_op, BuiltInOp, OperatorFn);
rt_value_access!(
    current_file,
    opt_current_file,
    CurrentFile,
    Rc<RefCell<CurrentFile<'a>>>
);

impl<'a> RuntimeValue<'a> {
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
    Name(Name),
}

impl<'a> TryFrom<RuntimeValue<'a>> for Key {
    type Error = MachineError;

    fn try_from(v: RuntimeValue) -> Result<Self, Self::Error> {
        match v {
            RuntimeValue::Value(Value::Bool(b)) => Ok(Self::Bool(b)),
            RuntimeValue::Value(Value::Integer(i)) => Ok(Self::Integer(i)),
            RuntimeValue::Value(Value::Name(n)) => Ok(Self::Name(n)),
            RuntimeValue::Value(Value::String(s)) => {
                Ok(Self::Name(name(from_utf8(&s.borrow()).unwrap())))
            }
            _ => Err(MachineError::TypeCheck),
        }
    }
}

impl From<TokenArray> for Value {
    fn from(v: TokenArray) -> Self {
        Self::Procedure(Rc::new(RefCell::new(v)))
    }
}

impl<'a> From<Value> for RuntimeValue<'a> {
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

static INVALID1: Name = Name::from_static("$$invalid1$$");
static INVALID2: Name = Name::from_static("$$invalid2$$");

impl std::borrow::Borrow<Name> for Key {
    fn borrow(&self) -> &Name {
        match self {
            // return a string that will never be a valid name to never select bool key using str
            Key::Bool(_) => &INVALID1,
            Key::Integer(_) => &INVALID2,
            Key::Name(n) => n,
        }
    }
}

pub type Dictionary = HashMap<Key, Value>;

#[derive(Debug, PartialEq, Clone)]
pub enum Token {
    Literal(Value),
    /// Name to lookup operation dict to get the actual operator
    Name(Name),
}

impl<'a> From<Token> for RuntimeValue<'a> {
    fn from(v: Token) -> Self {
        match v {
            Token::Literal(v) => Self::Value(v),
            Token::Name(name) => Self::Value(Value::Name(name)),
        }
    }
}

impl<'a> TryFrom<RuntimeValue<'a>> for Token {
    type Error = MachineError;

    fn try_from(v: RuntimeValue) -> Result<Self, Self::Error> {
        match v {
            RuntimeValue::Value(Value::Name(n)) => Ok(Self::Name(n)),
            RuntimeValue::Value(v) => Ok(Self::Literal(v)),
            RuntimeValue::Dictionary(d) => {
                let d: RuntimeDictionary =
                    Rc::try_unwrap(d).map_or_else(|d| d.borrow().clone(), |d| d.into_inner());
                Ok(Self::Literal(Value::Dictionary(into_dict(d)?)))
            }
            _ => Err(MachineError::TypeCheck),
        }
    }
}

#[cfg(test)]
pub fn name_token(s: &str) -> Token {
    Token::Name(name(s))
}

impl<'a> TryFrom<RuntimeValue<'a>> for Value {
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

        impl<'a> From<$t> for RuntimeValue<'a> {
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

impl<'a, const N: usize> From<[u8; N]> for RuntimeValue<'a> {
    fn from(v: [u8; N]) -> Self {
        let bytes: Vec<u8> = v.into();
        bytes.into()
    }
}

impl<'a> From<Vec<u8>> for RuntimeValue<'a> {
    fn from(v: Vec<u8>) -> Self {
        Value::String(Rc::new(RefCell::new(v))).into()
    }
}

impl From<Vec<u8>> for Value {
    fn from(v: Vec<u8>) -> Self {
        Self::String(Rc::new(RefCell::new(v)))
    }
}

impl<'a> From<&str> for RuntimeValue<'a> {
    fn from(v: &str) -> Self {
        Value::from(v).into()
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Self::Name(name(v))
    }
}

impl From<Array> for Value {
    fn from(v: Array) -> Self {
        Self::Array(Rc::new(RefCell::new(v)))
    }
}

impl<'a> From<Array> for RuntimeValue<'a> {
    fn from(v: Array) -> Self {
        Value::Array(Rc::new(RefCell::new(v))).into()
    }
}

impl<'a> From<RuntimeDictionary<'a>> for RuntimeValue<'a> {
    fn from(v: RuntimeDictionary<'a>) -> Self {
        RuntimeValue::Dictionary(Rc::new(RefCell::new(v)))
    }
}

impl<'a> From<Rc<RefCell<RuntimeDictionary<'a>>>> for RuntimeValue<'a> {
    fn from(v: Rc<RefCell<RuntimeDictionary<'a>>>) -> Self {
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
                Value::Dictionary(into_dict(d)?)
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
    #[allow(dead_code)]
    #[error("invalid access")]
    InvalidAccess,
    #[error("range check error")]
    RangeCheck,
}

pub type MachineResult<T> = Result<T, MachineError>;

struct CurrentFile<'a> {
    data: &'a [u8],
    remains_pos: usize,
    hex_form: bool,
    decryped: Option<Vec<u8>>,
    decryped_pos: usize,
}

impl<'a> CurrentFile<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            remains_pos: 0,
            decryped: None,
            decryped_pos: 0,
            hex_form: false,
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
        let decrypted;
        (self.hex_form, decrypted) = decrypt(EEXEC_KEY, 4, remains);
        self.decryped = Some(decrypted);
        self.remains_pos += 4;
        self.decryped_pos = 0;
    }

    pub fn stop_decrypt(&mut self) {
        assert!(self.decryped.is_some());
        self.skip_white_space();
        self.remains_pos += if self.hex_form {
            self.decryped_pos * 2
        } else {
            self.decryped_pos
        };
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
pub struct Machine<'a> {
    file: Rc<RefCell<CurrentFile<'a>>>,
    variable_stack: VariableDictStack<'a>,
    stack: Vec<RuntimeValue<'a>>,
    fonts: Vec<(String, Dictionary)>,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ExecState {
    Ok,
    // starts decrypt if exec() returns this
    StartEExec,
    // ends decrypt if exec() returns this
    EndEExec,
    DefinesEncoding,
}

impl<'a> Machine<'a> {
    pub fn new(file: &'a [u8]) -> Self {
        Self {
            file: Rc::new(RefCell::new(CurrentFile::new(file))),
            variable_stack: VariableDictStack::new(),
            stack: Vec::new(),
            fonts: vec![],
        }
    }

    pub fn exec_as_function(&mut self, args: &[f32], n_out: usize) -> MachineResult<Vec<f32>> {
        for arg in args.into_iter() {
            self.push(*arg);
        }

        self.execute()?;
        let mut r = vec![];
        match self.pop()? {
            RuntimeValue::Value(Value::Procedure(p)) => {
                // the function may wrapped in a procedure
                assert_eq!(ExecState::Ok, self.execute_procedure(p)?);
            }
            RuntimeValue::Value(Value::Real(v)) => r.push(v),
            RuntimeValue::Value(Value::Integer(v)) => r.push(v as f32),
            _ => return Err(MachineError::TypeCheck),
        }

        r.extend(
            self.stack
                .drain(..)
                .rev()
                .take(n_out)
                .map(|v| v.number().unwrap().map_left(|v| v as f32).into_inner()),
        );
        if r.len() != n_out {
            return Err(MachineError::StackUnderflow);
        }
        Ok(r)
    }

    #[allow(dead_code)]
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
                ExecState::DefinesEncoding => {}
            }
        }
        // assert that remains are all white space or comment
        self.file.borrow_mut().finish();

        Ok(())
    }

    /// Execute the type1 font PostScript until a Encoding defined,
    /// return the Encoding.
    pub fn execute_for_encoding(&mut self) -> MachineResult<Value> {
        // correct implement PostScript machine need too much work,
        // luckily encoding exist in very beginning

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
                ExecState::DefinesEncoding => {
                    return self
                        .variable_stack
                        .top()
                        .borrow_mut()
                        .remove(&sname("Encoding"))
                        .unwrap()
                        .try_into();
                }
            }
        }
        // assert that remains are all white space or comment
        self.file.borrow_mut().finish();
        Err(MachineError::Undefined)
    }

    #[allow(dead_code)]
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
                // debug!("{}", name);
                let v = self.variable_stack.get(&name)?;
                match v {
                    RuntimeValue::BuiltInOp(op) => op(self)?,
                    RuntimeValue::Value(Value::Procedure(p)) => self.execute_procedure(p)?,
                    RuntimeValue::Dictionary(d) => {
                        self.push(d);
                        ExecState::Ok
                    }
                    encoding @ RuntimeValue::Value(Value::PredefinedEncoding(_)) => {
                        self.push(encoding);
                        ExecState::Ok
                    }
                    v => unreachable!("{:?}", v),
                }
            }
        })
    }

    fn execute_procedure(&mut self, proc: Rc<RefCell<TokenArray>>) -> MachineResult<ExecState> {
        for token in proc.borrow().iter().cloned() {
            assert_eq!(
                self.exec(token)?,
                ExecState::Ok,
                "procedure should not return StartEExec or EndEExec"
            );
        }
        Ok(ExecState::Ok)
    }

    fn dump_stack(&self) {
        // debug!("{}", {
        //     use std::fmt::Write;
        //     let mut s = "stack: ".to_owned();
        //     for v in self.stack.iter().rev() {
        //         write!(&mut s, "{v} ").unwrap();
        //     }
        //     s
        // });
    }

    fn pop(&mut self) -> MachineResult<RuntimeValue<'a>> {
        let r = self.stack.pop().ok_or(MachineError::StackUnderflow);
        self.dump_stack();
        r
    }

    fn top(&self) -> MachineResult<&RuntimeValue<'a>> {
        self.stack.last().ok_or(MachineError::StackUnderflow)
    }

    fn push(&mut self, v: impl Into<RuntimeValue<'a>>) {
        self.stack.push(v.into());
        self.dump_stack();
    }

    fn push_current_file(&mut self) {
        self.push(RuntimeValue::CurrentFile(self.file.clone()))
    }

    fn define_font(&mut self, name: String, font: Dictionary) {
        self.fonts.push((name, font));
    }
}

struct VariableDictStack<'a> {
    stack: Vec<Rc<RefCell<RuntimeDictionary<'a>>>>,
}

macro_rules! built_in_ops {
    ($($k:expr => $v:expr),* $(,)?) => {
        std::iter::Iterator::collect(std::iter::IntoIterator::into_iter([$((Key::Name($k), RuntimeValue::BuiltInOp($v)),)*]))
    };
}

macro_rules! dict {
    () => {
        RuntimeDictionary::new()
    };
    ($($k:expr => $v:expr),* $(,)?) => {
        std::iter::Iterator::collect::<RuntimeDictionary>(std::iter::IntoIterator::into_iter([$((Key::Name($k), RuntimeValue::from($v)),)*]))
    };
}

fn ok() -> MachineResult<ExecState> {
    Ok(ExecState::Ok)
}

/// Create the `systemdict`
fn system_dict<'a>() -> RuntimeDictionary<'a> {
    let mut r: RuntimeDictionary<'a> = built_in_ops!(
        // any1 any2 exch -> any2 any1
        sname("exch") => (|m| {
            let a = m.pop()?;
            let b = m.pop()?;
            m.push(a);
            m.push(b);
            ok()
        }) as OperatorFn,

        // any -> any any
        sname("dup") => |m| {
            m.push(m.top()?.clone());
            ok()
        },
        // any pop -
        sname("pop") => |m| {
            m.pop()?;
            ok()
        },
        // Push counts of items in stack to stack
        sname("count") => |m| {
            let len: i32 = m.stack.len().try_into().unwrap();
            m.push(len);
            ok()
        },
        // any1 .. any(n) n copy any1 .. any(n) any1 .. any(n)
        sname("copy") => |m| {
            let count = m.pop()?.int().expect("merge dict/array/string not implemented");
            let mut items = Vec::new();
            for _ in 0..count {
                items.push(m.pop()?);
            }
            items.reverse();
            for item in &items {
                m.push(item.clone());
            }
            for item in items {
                m.push(item);
            }
            ok()
        },

        // Duplicate stack value at -n position
        // any(n) ... any0 n index -> any(n) ... any0 any(n)
        sname("index") => |m| {
            let index = m.pop()?.int()?;
            m.push(m.stack.get(m.stack.len() - index as usize - 1)
                .ok_or(MachineError::StackUnderflow)?
                .clone());
            ok()
        },

        // - mark -> Mark
        sname("mark") => |m| {
            m.push(RuntimeValue::Mark);
            ok()
        },
        // Mark obj1 .. obj(n) cleartomark -> -
        sname("cleartomark") => |m| {
            while m.pop()
                .map_err(|e| if e == MachineError::StackUnderflow {MachineError::UnMatchedMark } else {e})?
                 != RuntimeValue::Mark {}
            ok()
        },

        // - true -> true
        sname("true") => |m| {
            m.push(true);
            ok()
        },
        // - false -> false
        sname("false") => |m| {
            m.push(false);
            ok()
        },

        // bool1 bool2 and -> bool3
        // int1 int2 and -> int3
        sname("and") => |m| {
            let a = m.pop()?;
            let b = m.pop()?;
            match (a, b) {
                (RuntimeValue::Value(Value::Bool(a)), RuntimeValue::Value(Value::Bool(b))) => {
                    m.push(a && b)
                }
                (RuntimeValue::Value(Value::Integer(a)), RuntimeValue::Value(Value::Integer(b))) => {
                    m.push(a & b)
                }
                _ => return Err(MachineError::TypeCheck),
            }
            ok()
        },
        // bool1 bool2 or -> bool3
        // int1 int2 or -> int3
        sname("or") => |m| {
            let a = m.pop()?;
            let b = m.pop()?;
            match (a, b) {
                (RuntimeValue::Value(Value::Bool(a)), RuntimeValue::Value(Value::Bool(b))) => {
                    m.push(a || b)
                }
                (RuntimeValue::Value(Value::Integer(a)), RuntimeValue::Value(Value::Integer(b))) => {
                    m.push(a | b)
                }
                _ => return Err(MachineError::TypeCheck),
            }
            ok()
        },
        // bool1 not -> bool2
        // int1 not -> int2
        sname("not") => |m| {
            let v = m.pop()?;
            match v {
                RuntimeValue::Value(Value::Bool(b)) => m.push(!b),
                RuntimeValue::Value(Value::Integer(i)) => m.push(!i),
                _ => return Err(MachineError::TypeCheck),
            }
            ok()
        },
        // bool1 bool2 xor -> bool3
        // int1 int2 xor -> int3
        sname("xor") => |m| {
            let a = m.pop()?;
            let b = m.pop()?;
            match (a, b) {
                (RuntimeValue::Value(Value::Bool(a)), RuntimeValue::Value(Value::Bool(b))) => {
                    m.push(a ^ b)
                }
                (RuntimeValue::Value(Value::Integer(a)), RuntimeValue::Value(Value::Integer(b))) => {
                    m.push(a ^ b)
                }
                _ => return Err(MachineError::TypeCheck),
            }
            ok()
        },

        sname("eq") => |m| {
            let b = m.pop()?;
            let a = m.pop()?;
            m.push(object_eq(a, b));
            ok()
        },
        sname("ne") => |m| {
            let b = m.pop()?;
            let a = m.pop()?;
            m.push(!object_eq(a, b));
            ok()
        },
        // num1 num2 le -> bool
        // string1 string2 le -> bool
        sname("le") => |m| {
            let b = m.pop()?;
            let a = m.pop()?;
            m.push(!object_gt(&a, &b)? || object_eq(a, b));
            ok()
        },
        sname("lt") => |m| {
            let b = m.pop()?;
            let a = m.pop()?;
            m.push(!object_gt(&a, &b)? && !object_eq(a, b));
            ok()
        },
        sname("ge") => |m| {
            let b = m.pop()?;
            let a = m.pop()?;
            m.push(object_gt(&a, &b)? || object_eq(a, b));
            ok()
        },
        sname("gt") => |m| {
            let b = m.pop()?;
            let a = m.pop()?;
            m.push(object_gt(&a, &b)? && !object_eq(a, b));
            ok()
        },

        // num1 abs num1
        sname("abs") => |m| {
            let a = m.pop()?.number()?;
            match a {
                Either::Left(a) => m.push(a.abs()),
                Either::Right(a) => m.push(a.abs()),
            }
            ok()
        },

        // num1 num2 add sum
        sname("add") => |m| {
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

        // num1 num2 sub difference
        sname("sub") => |m| {
            let b = m.pop()?.number()?;
            let a = m.pop()?.number()?;
            match (a, b) {
                (Either::Left(a), Either::Left(b)) => m.push(a - b),
                (Either::Right(a), Either::Right(b)) => m.push(a - b),
                (Either::Left(a), Either::Right(b)) => m.push(a as f32 - b),
                (Either::Right(a), Either::Left(b)) => m.push(a - b as f32),
            }
            ok()
        },

        // num1 num2 mul num3
        sname("mul") => |m| {
            let a = m.pop()?.number()?;
            let b = m.pop()?.number()?;
            match (a, b) {
                (Either::Left(a), Either::Left(b)) => m.push(a * b),
                (Either::Right(a), Either::Right(b)) => m.push(a * b),
                (Either::Left(a), Either::Right(b)) => m.push(a as f32 * b),
                (Either::Right(a), Either::Left(b)) => m.push(a * b as f32),
            }
            ok()
        },

        // num1 neg num2
        sname("neg") => |m| {
            let a = m.pop()?.number()?;
            match a {
                Either::Left(a) => m.push(-a),
                Either::Right(a) => m.push(-a),
            }
            ok()
        },

        // num1 ceiling num2
        sname("ceiling") => |m| {
            let a = m.pop()?.number()?;
            match a {
                Either::Left(a) => m.push(a),
                Either::Right(a) => m.push(a.ceil()),
            }
            ok()
        },

        // num1 floor num2
        sname("floor") => |m| {
            let a = m.pop()?.number()?;
            match a {
                Either::Left(a) => m.push(a),
                Either::Right(a) => m.push(a.floor()),
            }
            ok()
        },

        // num1 round round2
        sname("round") => |m| {
            let a = m.pop()?.number()?;
            match a {
                Either::Left(a) => m.push(a),
                Either::Right(a) => m.push(a.round()),
            }
            ok()
        },

        // int1 int2 idiv quotient
        sname("idiv") => |m| {
            let b = m.pop()?.int()?;
            let a = m.pop()?.int()?;
            m.push(a / b);
            ok()
        },

        // int1 int2 mod remainder
        sname("mod") => |m| {
            let b = m.pop()?.int()?;
            let a = m.pop()?.int()?;
            m.push(a % b);
            ok()
        },

        // int array -> array
        sname("array") => |m| {
            let count = m.pop()?.int()?;
            m.push(Array::from_iter(repeat(Value::Null).take(count as usize)));
            ok()
        },
        sname("[") => |m| {
            m.push(RuntimeValue::ArrayMark);
            ok()
        },
        sname("]") => |m| {
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
        sname("[]") => |m| {
            m.push(Array::new());
            ok()
        },

        // int dict -> dict
        sname("dict") => |m| {
            let count = m.pop()?.int()?;
            m.push(RuntimeDictionary::with_capacity(count as usize));
            ok()
        },
        sname("<<") => |m| {
            m.push(RuntimeValue::DictMark);
            ok()
        },
        sname(">>") => |m| {
            let mut dict = RuntimeDictionary::new();
            loop {
                let v = match m.pop()? {
                    RuntimeValue::DictMark => {
                        m.push(dict);
                        return ok();
                    }
                    v => v
                };
                let key = m.pop()?;
                dict.insert(key.try_into()?, v);
            }
        },

        // dict begin -> -
        sname("begin") => |m| {
            let dict = m.pop()?.dict()?;
            m.variable_stack.push(dict);
            ok()
        },

        // - end -> -
        sname("end") => |m| {
            m.variable_stack.pop();
            ok()
        },

        // key value -> - Set key-value to current directory.
        sname("def") => |m| {
            let value = m.pop()?;
            let key = m.pop()?;
            let dict = m.variable_stack.top();
            let is_encoding = if let RuntimeValue::Value(Value::Name(ref name)) = key {
                name == &sname("Encoding")
            } else {
                false
            };
            dict.borrow_mut().insert(key.try_into()?, value);
            if is_encoding {
                return Ok(ExecState::DefinesEncoding);
            }
            ok()
        },

        // dict key known -> bool
        sname("known") => |m| {
            let key = m.pop()?;
            let dict = m.pop()?.dict()?;
            let key: Key = key.try_into()?;
            let r = dict.borrow().contains_key(&key);
            m.push(r);
            ok()
        },

        // array  index put -> -
        // dict   key   put -> -
        // string index get -> -
        sname("put") => |m| {
            let value = m.pop()?;
            let key = m.pop()?;
            match m.pop()? {
                RuntimeValue::Dictionary(dict) => {
                    let key: Key = key.try_into()?;
                    dict.borrow_mut().insert(key, value);
                }
                RuntimeValue::Value(Value::Array(array)) => {
                    let index = key.int()?;
                    let mut array = array.borrow_mut();
                    let index = index as usize;
                    if index >= array.len() {
                        return Err(MachineError::RangeCheck);
                    }
                    array[index] = value.try_into()?;
                }
                RuntimeValue::Value(Value::String(s)) =>
                {
                    let index = key.int()?;
                    let mut s = s.borrow_mut();
                    let index = index as usize;
                    if index >= s.len() {
                        return Err(MachineError::RangeCheck);
                    }
                    #[allow(clippy::cast_possible_truncation)]
                    {
                        s[index] = value.int()? as u8;
                    }
                }
                RuntimeValue::Value(Value::Procedure(arr)) => {
                    let index = key.int()?;
                    let mut arr = arr.borrow_mut();
                    let index = index as usize;
                    if index >= arr.len() {
                        return Err(MachineError::RangeCheck);
                    }
                    arr[index] = value.try_into()?;
                }
                v => {
                    error!("put on non-dict/array/string: {:?}, key: {:?}, value: {:?}", v, key, value);
                    return Err(MachineError::TypeCheck);
                }
            };
            ok()
        },
        // array  index get -> any
        // dict   key   get -> any
        // string index get -> int
        sname("get") => |m| {
            let key = m.pop()?;
            match m.pop()? {
                RuntimeValue::Dictionary(dict) => {
                    let key: Key = key.try_into()?;
                    let v = dict.borrow().get(&key).cloned().ok_or(MachineError::Undefined)?;
                    m.push(v);
                }
                RuntimeValue::Value(Value::Array(array)) => {
                    let index = key.int()?;
                    let array = array.borrow();
                    let v = array.get(index as usize).cloned().ok_or(MachineError::RangeCheck)?;
                    m.push(v);
                }
                RuntimeValue::Value(Value::Procedure(p)) => {
                    let index = key.int()?;
                    let v = p.borrow().get(index as usize).cloned().ok_or(MachineError::RangeCheck)?;
                    m.push(v);
                }
                RuntimeValue::Value(Value::String(s)) => {
                    let index = key.int()?;
                    let s = s.borrow();
                    let v = s.get(index as usize).cloned().ok_or(MachineError::RangeCheck)?;
                    m.push(v as i32);
                }
                v => {
                    error!("get on non-dict/array/string: {:?}, key: {:?}", v, key);
                    return Err(MachineError::TypeCheck);
                }
            };
            ok()
        },

        // int string -> string
        sname("string") => |m| {
            let count = m.pop()?.int()?;
            m.push(vec![0u8; count as usize]);
            ok()
        },

        // push current variable stack to operand stack
        sname("currentdict") => |m| {
            m.push(m.variable_stack.top());
            ok()
        },
        // push systemdict to operand stack
        sname("systemdict") => |m| {
            m.push(m.variable_stack.stack[0].clone());
            ok()
        },
        sname("userdict") => |m| {
            m.push(m.variable_stack.stack[2].clone());
            ok()
        },
        sname("currentfile") => |m| {
            m.push_current_file();
            ok()
        },
        sname("readstring") => |m| {
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
        sname("for") => |m| {
            let proc = m.pop()?.procedure()?;
            let limit = m.pop()?.int()?;
            let increment = m.pop()?.int()?;
            let initial = m.pop()?.int()?;
            for i in (initial..=limit).step_by(increment as usize) {
                m.push(i);
                m.execute_procedure(proc.clone())?;
            }
            ok()
        },
        // bool proc if-> -
        sname("if") => |m| {
            let proc = m.pop()?.procedure()?;
            let cond = m.pop()?.bool()?;
            if cond {
                m.execute_procedure(proc)?;
            }
            ok()
        },
        // bool proc1 proc2 ifelse -> -
        sname("ifelse") => |m| {
            let proc2 = m.pop()?.procedure()?;
            let proc1 = m.pop()?.procedure()?;
            let cond = m.pop()?.bool()?;
            m.execute_procedure(if cond { proc1 } else { proc2 })?;
            ok()
        },
        sname("eexec") => |m| {
            assert!(
                matches!(m.pop()?, RuntimeValue::CurrentFile(_)),
                "eexec on non-current file not implemented"
            );
            m.variable_stack.push_system_dict();
            Ok(ExecState::StartEExec)
        },
        sname("exec") => |m| {
            let proc = m.pop()?;
            match proc {
                RuntimeValue::Value(Value::Procedure(p)) => m.execute_procedure(p),
                v@RuntimeValue::Dictionary(_) => {m.push(v); ok()}
                _ => Err(MachineError::TypeCheck),
            }
        },
        // file closefile -
        sname("closefile") => |m| {
            let RuntimeValue::CurrentFile(_f) = m.pop()? else {
                return Err(MachineError::TypeCheck);
            };
            Ok(ExecState::EndEExec)
        },
        sname("definefont") => |m| {
            let font = m.pop()?;
            let key = m.pop()?;
            let name = key.name()?;
            m.define_font(name.as_str().to_owned(), into_dict(font.dict()?.borrow().clone())?);
            m.push(font);
            ok()
        },

        sname("readonly") => |_| ok(),
        sname("executeonly") => |_| ok(),
        sname("noaccess") => |_| ok(),
        sname("bind") => |_| {
            error!("bind not implemented");
            ok()
        },
        // any type -> name
        sname("type") => |m| {
            let v = m.pop()?;
            // TODO: font-type, g-state-type, packed-array-type, save-type
            m.push(match v {
                RuntimeValue::Value(Value::Bool(_)) => sname("booleantype"),
                RuntimeValue::Value(Value::Integer(_)) => sname("integertype"),
                RuntimeValue::Value(Value::Real(_)) => sname("realtype"),
                RuntimeValue::Value(Value::String(_)) => sname("stringtype"),
                RuntimeValue::Value(Value::Name(_)) => sname("nametype"),
                RuntimeValue::Value(Value::Array(_)) => sname("arraytype"),
                RuntimeValue::Dictionary(_) => sname("dicttype"),
                RuntimeValue::Value(Value::Procedure(_)) => sname("arraytype"),
                RuntimeValue::Value(Value::PredefinedEncoding(_)) => sname("arraytype"),
                RuntimeValue::CurrentFile(_) => sname("filetype"),
                RuntimeValue::BuiltInOp(_) => sname("operatortype"),
                RuntimeValue::Mark => sname("marktype"),
                RuntimeValue::ArrayMark => sname("marktype"),
                RuntimeValue::DictMark => sname("marktype"),
                RuntimeValue::Value(Value::Null) => sname("nulltype"),
                RuntimeValue::Value(Value::Dictionary(_)) => sname("dicttype"),
            });
            ok()
        },

        // key category findresource - instance
        sname("findresource") => |m| {
            let category = m.pop()?.name()?;
            let key = m.pop()?.name()?;
            assert_eq!(key.as_ref(), "CIDInit");
            assert_eq!(category.as_ref(), "ProcSet");
            let cid_init = Rc::new(RefCell::new(cidinit::cid_init_dict()));
            m.variable_stack.push(cid_init.clone());
            m.push(cid_init);
            ok()
        }
    );

    r.insert(
        Key::Name(sname("StandardEncoding")),
        RuntimeValue::Value(Value::PredefinedEncoding(sname("StandardEncoding"))),
    );
    r.insert(
        Key::Name(sname("internaldict")),
        RuntimeValue::Dictionary(Rc::new(RefCell::new(RuntimeDictionary::new()))),
    );
    r
}

fn object_gt<'a>(a: &RuntimeValue<'a>, b: &RuntimeValue<'a>) -> MachineResult<bool> {
    Ok(match (a, b) {
        (RuntimeValue::Value(Value::Integer(a)), RuntimeValue::Value(Value::Integer(b))) => a > b,
        (RuntimeValue::Value(Value::Real(a)), RuntimeValue::Value(Value::Real(b))) => a > b,
        (RuntimeValue::Value(Value::Integer(a)), RuntimeValue::Value(Value::Real(b))) => {
            *a as f32 > *b
        }
        (RuntimeValue::Value(Value::Real(a)), RuntimeValue::Value(Value::Integer(b))) => {
            *a > *b as f32
        }
        (RuntimeValue::Value(Value::String(a)), RuntimeValue::Value(Value::String(b))) => {
            a.borrow().as_slice() > b.borrow().as_slice()
        }
        _ => return Err(MachineError::TypeCheck),
    })
}

fn object_eq<'a>(a: RuntimeValue<'a>, b: RuntimeValue<'a>) -> bool {
    match (a, b) {
        (RuntimeValue::Value(Value::Integer(a)), RuntimeValue::Value(Value::Real(b))) => {
            a as f32 == b
        }
        (RuntimeValue::Value(Value::Real(a)), RuntimeValue::Value(Value::Integer(b))) => {
            a == b as f32
        }
        (RuntimeValue::Value(Value::String(a)), RuntimeValue::Value(Value::Name(b))) => {
            a.borrow().as_slice() == b.as_ref().as_bytes()
        }
        (RuntimeValue::Value(Value::Name(a)), RuntimeValue::Value(Value::String(b))) => {
            b.borrow().as_slice() == a.as_ref().as_bytes()
        }
        (RuntimeValue::Value(Value::Name(a)), RuntimeValue::Value(Value::Name(b))) => a == b,
        (RuntimeValue::Value(Value::Array(a)), RuntimeValue::Value(Value::Array(b))) => {
            Rc::ptr_eq(&a, &b)
        }
        (RuntimeValue::Dictionary(a), RuntimeValue::Dictionary(b)) => Rc::ptr_eq(&a, &b),
        (RuntimeValue::Value(Value::Procedure(a)), RuntimeValue::Value(Value::Procedure(b))) => {
            Rc::ptr_eq(&a, &b)
        }
        (a, b) => a == b,
    }
}

/// Create the `globaldict`
fn global_dict<'a>() -> RuntimeDictionary<'a> {
    dict![
        sname("FontDirectory") => RuntimeDictionary::new(),
    ]
}

/// Create the `userdict`
fn user_dict<'a>() -> RuntimeDictionary<'a> {
    RuntimeDictionary::new()
}

impl<'a> VariableDictStack<'a> {
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

    fn get(&self, name: &Name) -> MachineResult<RuntimeValue<'a>> {
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

    fn push(&mut self, dict: Rc<RefCell<RuntimeDictionary<'a>>>) {
        self.stack.push(dict);
    }

    /// Pop the top dictionary from the stack. The first 3 dictionaries can not
    /// be popped, returns None if trying to pop them.
    fn pop(&mut self) -> Option<Rc<RefCell<RuntimeDictionary<'a>>>> {
        (self.stack.len() > 3).then(|| self.stack.pop()).flatten()
    }

    fn top(&self) -> Rc<RefCell<RuntimeDictionary<'a>>> {
        self.stack.last().unwrap().clone()
    }

    fn lock_system_dict(&self) -> Ref<RuntimeDictionary<'a>> {
        self.stack[0].borrow()
    }
}

#[cfg(test)]
mod tests;
