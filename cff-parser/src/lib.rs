//! Decode Adobe CFF font files.
//!
//! The entry type is `File`, use `File::open()` method
//! to parses a CFF file and returns `File`
//!
//! File is a struct that contains all the data in a CFF file.
//! derived types borrows data hold by File.
//!
//! File::fonts() returns a iterator of `Font` which is a struct
//! that provides info for that font, such as encoding, charset, etc.
mod inner;

pub use inner::{Error, Result};

pub struct Font<'a> {
    file: &'a [u8],
    idx: u8,
    name: &'a str,
}

impl<'a> Font<'a> {
    pub fn new(file: &'a [u8], idx: u8, name: &'a str) -> Self {
        Self { file, idx, name }
    }

    pub fn name(&self) -> &str {
        self.name
    }
}

/// Iterator of Font.
pub struct Fonts<'a> {
    f: &'a File,
    names: inner::NameIndex<'a>,
    idx: usize,
}

impl<'a> Fonts<'a> {
    pub fn new(f: &'a File) -> Result<Self> {
        let names_offset = f.header.hdr_size as usize;
        Ok(Self {
            f,
            names: inner::parse_name_index(&f.data[names_offset..])?.1,
            idx: 0,
        })
    }
}

impl<'a> Iterator for Fonts<'a> {
    type Item = Font<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx < self.names.len() {
            let name = self.names.get(self.idx);
            self.idx += 1;
            match name {
                Some(name) => Some(Font::new(&self.f.data, self.idx as u8, name)),
                None => self.next(),
            }
        } else {
            None
        }
    }
}

pub struct File {
    header: inner::Header,
    data: Vec<u8>,
}

impl File {
    pub fn open(data: Vec<u8>) -> Result<Self> {
        debug_assert_eq!(data.len(), data.capacity());

        let (_, header) = inner::parse_header(&data)?;
        Ok(File { data, header })
    }

    pub fn iter(&self) -> Result<Fonts<'_>> {
        Fonts::new(&self)
    }

    pub fn major_version(&self) -> u8 {
        self.header.major
    }

    pub fn minor_version(&self) -> u8 {
        self.header.minor
    }
}

#[cfg(test)]
mod tests;
