use std::collections::HashMap;

type Dictionary<'a> = HashMap<Name<'a>, Object<'a>>;
type Array<'a> = Vec<Object<'a>>;

pub enum Object<'a> {
    Null,
    Boolean(bool),
    Integer(i32),
    Number(f32),
    String(&'a [u8]),
    Name(Name<'a>), // without the leading slash
    Dictionary(Dictionary<'a>),
    Array(Array<'a>),
    Stream((Dictionary<'a>, &'a [u8])), // data part not including the stream/endstream keyword
    Reference(Reference),
}

#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub struct Name<'a>(&'a [u8]);

#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub struct Reference {
    pub id: u32,
    pub generation: u16,
}
