use std::fmt::{Display, Write};

use super::object::Utf8OrHexDumper;
use super::Indent;
use istring::small::SmallString;
use pdf::{
    object::PlainRef,
    primitive::{Dictionary, PdfStream, Primitive},
};

pub struct PrimitiveDumper<'a>(&'a Primitive, Indent);

impl<'a> PrimitiveDumper<'a> {
    pub fn new(p: &'a Primitive) -> Self {
        Self(p, Indent(0))
    }

    fn with_indent(p: &'a Primitive, indent: Indent) -> Self {
        Self(p, indent)
    }
}

impl<'a> Display for PrimitiveDumper<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            Primitive::Null => f.write_str("null"),
            Primitive::Integer(i) => f.write_fmt(format_args!("{}", i)),
            Primitive::Number(r) => f.write_fmt(format_args!("{}", r)),
            Primitive::Boolean(b) => f.write_str(if *b { "true" } else { "false" }),
            Primitive::String(s) => {
                f.write_fmt(format_args!("({})", Utf8OrHexDumper(s.as_bytes())))
            }
            Primitive::Stream(s) => StreamDumper::with_indent(s, self.1).fmt(f),
            Primitive::Dictionary(d) => DictionaryDumper::with_indent(d, self.1).fmt(f),
            Primitive::Array(a) => ArrayDumper::with_indent(a, self.1).fmt(f),
            Primitive::Reference(r) => PlainRefDumper(r).fmt(f),
            Primitive::Name(n) => NameDumper(n).fmt(f),
        }
    }
}

struct NameDumper<'a>(&'a SmallString);

impl<'a> Display for NameDumper<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("/{}", self.0))
    }
}

struct PlainRefDumper<'a>(&'a PlainRef);

impl<'a> Display for PlainRefDumper<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{} {} R", self.0.id, self.0.gen))
    }
}

struct StreamDumper<'a>(&'a PdfStream, Indent);

impl<'a> StreamDumper<'a> {
    fn with_indent(s: &'a PdfStream, indent: Indent) -> Self {
        Self(s, indent)
    }
}

impl<'a> Display for StreamDumper<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("STREAM\n")?;
        self.1.inc().fmt(f)?;
        DictionaryDumper::with_indent(&self.0.info, self.1.inc()).fmt(f)
    }
}

pub struct ArrayDumper<'a>(&'a [Primitive], Indent);

impl<'a> ArrayDumper<'a> {
    pub fn new(a: &'a [Primitive]) -> Self {
        Self(a, Indent(0))
    }

    fn with_indent(a: &'a [Primitive], indent: Indent) -> Self {
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
                PrimitiveDumper::with_indent(item, indent).fmt(f)?;
                f.write_char('\n')
            })?;
            self.1.fmt(f)?;
        } else {
            for (i, item) in self.0.iter().enumerate() {
                if i > 0 {
                    f.write_char(' ')?;
                }
                PrimitiveDumper::with_indent(item, indent).fmt(f)?;
            }
        }

        f.write_char(']')
    }
}

struct DictionaryDumper<'a>(&'a Dictionary, Indent);

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
        f.write_str("<<")?;

        let indent = self.1.inc();
        if !is_dictionary_complex(self.0) {
            for (k, v) in self.0.iter() {
                f.write_fmt(format_args!(
                    "{} {}",
                    NameDumper(&k.0),
                    PrimitiveDumper::with_indent(v, indent)
                ))?;
            }
        } else {
            f.write_char('\n')?;
            for (k, v) in self.0.iter() {
                indent.fmt(f)?;
                NameDumper(&k.0).fmt(f)?;
                if !is_complex_primitive(v) {
                    f.write_char(' ')?;
                } else {
                    f.write_fmt(format_args!("\n{}", indent))?;
                }
                PrimitiveDumper::with_indent(v, indent).fmt(f)?;
                f.write_char('\n')?;
            }
            self.1.fmt(f)?;
        }
        f.write_str(">>")
    }
}

fn is_array_complex(a: &[Primitive]) -> bool {
    if a.len() > 3 {
        true
    } else {
        a.iter().any(is_complex_primitive)
    }
}

fn is_dictionary_complex(d: &Dictionary) -> bool {
    if d.len() > 1 {
        true
    } else {
        d.iter()
            .next()
            .map(|(_, v)| is_complex_primitive(v))
            .unwrap_or(false)
    }
}

fn is_complex_primitive(p: &Primitive) -> bool {
    match p {
        Primitive::Dictionary(dict) => is_dictionary_complex(dict),
        Primitive::Array(items) => is_array_complex(items),
        _ => false,
    }
}

#[cfg(test)]
mod tests;
