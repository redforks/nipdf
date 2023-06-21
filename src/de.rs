use crate::object::{Object, ObjectValueError};
use serde::{de, forward_to_deserialize_any, Deserialize, Deserializer};

type Result<T> = std::result::Result<T, ObjectValueError>;

pub fn from_object<'a, 'de: 'a, T: Deserialize<'a>>(o: &'de Object<'a>) -> Result<T> {
    let mut deserilizer = ObjectDeserializer::new(o);
    T::deserialize(&mut deserilizer)
}

#[derive(Debug, PartialEq)]
pub struct ObjectDeserializer<'a, 'de> {
    object: &'de Object<'a>,
}

impl<'a, 'de, 'b> Deserializer<'de> for &'b mut ObjectDeserializer<'a, 'de> {
    type Error = ObjectValueError;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        match self.object {
            Object::Bool(v) => visitor.visit_bool(*v),
            _ => todo!(),
        }
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

impl<'a, 'b> ObjectDeserializer<'a, 'b> {
    pub fn new(object: &'b Object<'a>) -> Self {
        Self { object }
    }
}

#[cfg(test)]
mod tests;
