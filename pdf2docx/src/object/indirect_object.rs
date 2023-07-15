use super::{Object, ObjectId};

#[derive(Debug, Clone, PartialEq)]
pub struct IndirectObject<'a>(ObjectId, Object<'a>);

impl<'a> IndirectObject<'a> {
    pub fn new(id: u32, generation: u16, object: Object<'a>) -> Self {
        Self(ObjectId::new(id, generation), object)
    }

    pub fn id(&self) -> ObjectId {
        self.0
    }

    pub fn object(&self) -> &Object<'a> {
        &self.1
    }

    pub fn take(self) -> Object<'a> {
        self.1
    }
}
