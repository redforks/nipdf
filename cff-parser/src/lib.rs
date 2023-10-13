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
}

impl<'a> Font<'a> {
    pub fn name(&self) -> Result<&str> {
        todo!()
    }
}

/// Iterator of Font.
pub struct Fonts<'a>(&'a File);

impl<'a> Iterator for Fonts<'a> {
    type Item = Font<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        todo!()
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

    pub fn iter(&self) -> Fonts<'_> {
        Fonts(&self)
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
