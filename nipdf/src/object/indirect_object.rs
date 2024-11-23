use super::{Object, ObjectId};

#[derive(Debug, Clone, PartialEq)]
pub struct IndirectObjectDef(pub(crate) ObjectId, pub(crate) Object);

impl IndirectObjectDef {
    pub fn new(id: u32, generation: u16, object: Object) -> Self {
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
