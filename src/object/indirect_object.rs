use super::Object;

pub struct IndirectObject<'a> {
    id: u32,
    generation: u16,
    object: Object<'a>,
}

impl<'a> IndirectObject<'a> {
    pub fn new(id: u32, generation: u16, object: Object<'a>) -> Self {
        Self {
            id,
            generation,
            object,
        }
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn generation(&self) -> u16 {
        self.generation
    }

    pub fn object(&self) -> &Object<'a> {
        &self.object
    }
}
