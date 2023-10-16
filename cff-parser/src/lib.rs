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

const NOTDEF: &str = ".notdef";
mod inner;

pub use inner::{Error, Result};

pub struct Font<'a> {
    font_data: &'a [u8],
    name: &'a str,
    top_dict_data: inner::TopDictData<'a>,
}

impl<'a> Font<'a> {
    pub fn new(font_data: &'a [u8], name: &'a str, top_dict_data: inner::TopDictData<'a>) -> Self {
        Self {
            font_data,
            name,
            top_dict_data,
        }
    }

    pub fn name(&self) -> &str {
        self.name
    }

    pub fn encodings(&self) -> Result<[&'a str; 256]> {
        let charsets = self.top_dict_data.charsets(self.font_data)?;
        let (encodings, supplements) = self.top_dict_data.encodings(self.font_data)?;
        let mut r = encodings.build(&charsets, self.top_dict_data.string_index());
        if let Some(supplements) = supplements {
            for supp in supplements {
                supp.apply(self.top_dict_data.string_index(), &mut r);
            }
        }

        Ok(r)
    }
}

/// Iterator of Font.
pub struct Fonts<'a> {
    data: &'a [u8],
    names_index: inner::NameIndex<'a>,
    top_dict_index: inner::TopDictIndex<'a>,
    string_index: inner::StringIndex<'a>,
    idx: usize,
}

impl<'a> Fonts<'a> {
    pub fn new(f: &File<'a>) -> Result<Self> {
        let names_offset = f.header.hdr_size as usize;
        let buf = &f.data[names_offset..];
        let (buf, names_index) = inner::parse_name_index(buf)?;
        let (buf, top_dict_index) = inner::parse_top_dict_index(buf)?;
        let (_, string_index) = inner::parse_string_index(buf)?;
        Ok(Self {
            data: f.data,
            names_index,
            top_dict_index,
            string_index,
            idx: 0,
        })
    }
}

impl<'a> Iterator for Fonts<'a> {
    type Item = Font<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx < self.names_index.len() {
            let name = self.names_index.get(self.idx);
            let top_dict_data = self.top_dict_index.get(self.idx, self.string_index).ok()?;
            self.idx += 1;
            match name {
                Some(name) => Some(Font::new(self.data, name, top_dict_data)),
                None => self.next(),
            }
        } else {
            None
        }
    }
}

pub struct File<'a> {
    header: inner::Header,
    data: &'a [u8],
}

impl<'a> File<'a> {
    pub fn open(data: &'a [u8]) -> Result<Self> {
        let (_, header) = inner::parse_header(data)?;
        Ok(File { data, header })
    }

    pub fn iter(&self) -> Result<Fonts<'a>> {
        Fonts::new(self)
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
