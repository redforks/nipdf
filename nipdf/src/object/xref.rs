use super::RuntimeObjectId;

#[derive(Debug, Clone, PartialEq, Copy)]
pub struct FilePos(u32, u16, bool); // offset, generation, is_used

impl FilePos {
    pub fn new(offset: u32, generation: u16, is_used: bool) -> Self {
        Self(offset, generation, is_used)
    }

    pub fn offset(&self) -> u32 {
        self.0
    }

    pub fn generation(&self) -> u16 {
        self.1
    }

    pub fn is_used(&self) -> bool {
        self.2
    }
}

#[derive(Debug, Clone, PartialEq, Copy)]
pub enum Entry {
    /// The object stored directly in file as object
    InFile(FilePos),
    /// The object stored in a stream, (stream_object_id, idx_of_object_in_stream)
    InStream(RuntimeObjectId, u16),
}

impl Entry {
    pub fn in_file(offset: u32, generation: u16, is_used: bool) -> Self {
        Self::InFile(FilePos::new(offset, generation, is_used))
    }

    pub fn in_stream(stream_object_id: RuntimeObjectId, idx_of_object_in_stream: u16) -> Self {
        Self::InStream(stream_object_id, idx_of_object_in_stream)
    }

    pub fn is_used(self) -> bool {
        match self {
            Entry::InFile(pos) => pos.is_used(),
            Entry::InStream(_, _) => true,
        }
    }
}

pub type Section = Vec<(u32, Entry)>;
