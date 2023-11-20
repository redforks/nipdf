use super::{Object, ObjectId};
use std::num::NonZeroU32;

#[derive(Debug, Clone, PartialEq)]
pub struct IndirectObject(ObjectId, Object);

impl<'a> IndirectObject {
    pub fn new(id: NonZeroU32, generation: u16, object: Object) -> Self {
        Self(ObjectId::new(id, generation), object)
    }

    pub fn id(&self) -> ObjectId {
        self.0
    }

    pub fn object(&self) -> &Object {
        &self.1
    }

    pub fn take(self) -> Object {
        self.1
    }
}
