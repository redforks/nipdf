//! Function and Types to dump `lopdf::Object` enum values
use std::fmt::Display;
use std::fmt::Write;

use super::Indent;
use lopdf::{Dictionary, Object, ObjectId, Stream};

pub struct ObjectIdDumper<'a>(&'a ObjectId);

impl<'a> ObjectIdDumper<'a> {
    pub fn new(id: &'a ObjectId) -> Self {
        Self(id)
    }
}

impl<'a> Display for ObjectIdDumper<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{} {} R", self.0 .0, self.0 .1))
    }
}

pub struct ObjectDumper<'a>(&'a Object, Indent);

impl<'a> ObjectDumper<'a> {
    pub fn new(obj: &'a Object) -> Self {
        Self(obj, Indent(0))
    }

    fn with_indent(obj: &'a Object, indent: Indent) -> Self {
        Self(obj, indent)
    }
}

impl<'a> Display for ObjectDumper<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}

pub struct ArrayDumper<'a>(&'a [Object], Indent);

impl<'a> ArrayDumper<'a> {
    pub fn new(a: &'a [Object]) -> Self {
        Self(a, Indent(0))
    }

    fn with_indent(a: &'a [Object], indent: Indent) -> Self {
        Self(a, indent)
    }
}

impl<'a> Display for ArrayDumper<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_char('[')?;

        let indent = self.1.inc();
        if is_array_complex(self.0) {
            f.write_char('\n')?;
            self.0.iter().try_for_each(|item| {
                indent.fmt(f)?;
                ObjectDumper::with_indent(item, indent).fmt(f)?;
                f.write_char('\n')
            })?;
            self.1.fmt(f)?;
        } else {
            for (i, item) in self.0.iter().enumerate() {
                if i > 0 {
                    f.write_char(' ')?;
                }
                ObjectDumper::with_indent(item, indent).fmt(f)?;
            }
        }

        f.write_char(']')
    }
}

pub struct DictionaryDumper<'a>(&'a Dictionary, Indent);

impl<'a> DictionaryDumper<'a> {
    pub fn new(d: &'a Dictionary) -> Self {
        Self(d, Indent(0))
    }

    fn with_indent(d: &'a Dictionary, indent: Indent) -> Self {
        Self(d, indent)
    }
}

impl<'a> Display for DictionaryDumper<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}

pub struct StreamDumper<'a>(&'a Stream, Indent);

impl<'a> StreamDumper<'a> {
    pub fn new(s: &'a Stream) -> Self {
        Self(s, Indent(0))
    }

    fn with_indent(s: &'a Stream, indent: Indent) -> Self {
        Self(s, indent)
    }
}

impl<'a> Display for StreamDumper<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.1.fmt(f)?;
        f.write_str("STREAM ")?;
        f.write_str(if self.0.allows_compression {
            "allows_compression"
        } else {
            "not allows_compression"
        })?;
        if let Some(pos) = self.0.start_position {
            f.write_fmt(format_args!(" @{}", pos))?;
        }
        f.write_char('\n')?;
        let indent = self.1.inc();
        indent.fmt(f)?;
        DictionaryDumper::with_indent(&self.0.dict, indent).fmt(f)
    }
}

fn is_array_complex(v: &[Object]) -> bool {
    if v.len() > 3 {
        true
    } else {
        v.iter().any(is_complex_pdf_value)
    }
}

fn is_dictionary_complex(v: &Dictionary) -> bool {
    if v.iter().count() > 1 {
        true
    } else {
        v.iter()
            .next()
            .map(|(_, v)| is_complex_pdf_value(v))
            .unwrap_or(false)
    }
}

/// Return true if `v` is Dictionary or Array
fn is_complex_pdf_value(v: &Object) -> bool {
    match v {
        Object::Dictionary(dict) => is_dictionary_complex(dict),
        Object::Array(items) => is_array_complex(items),
        _ => false,
    }
}

#[cfg(test)]
mod tests;
