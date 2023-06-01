//! Function and Types to dump `lopdf::Object` enum values
use std::fmt::Display;
use std::fmt::Write;
use std::str::from_utf8;

use lopdf::Dictionary;
use lopdf::Object;

struct HexDumer<'a>(&'a [u8]);

impl<'a> Display for HexDumer<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&hex::encode_upper(self.0))
    }
}

#[derive(Clone, Copy)]
/// When display, render n*2 spaces
struct Indent(usize);

impl Indent {
    fn inc(self) -> Self {
        Self(self.0 + 1)
    }

    fn dec(self) -> Self {
        debug_assert!(self.0 > 0);
        Self(self.0 - 1)
    }
}

impl Display for Indent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for _ in 0..(self.0 * 2) {
            f.write_char(' ')?;
        }
        Ok(())
    }
}

/// Dump `[u8]` as utf8 str, or hex if not valid utf8
struct Utf8OrHexDumper<'a>(&'a [u8]);

impl<'a> Display for Utf8OrHexDumper<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match from_utf8(self.0) {
            Ok(s) => f.write_fmt(format_args!("{}", s)),
            Err(_) => {
                f.write_str("0x")?;
                HexDumer(self.0).fmt(f)
            }
        }
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
        match self.0 {
            Object::Null => f.write_str("null"),
            Object::Boolean(b) => f.write_str(if *b { "true" } else { "false" }),
            Object::Integer(i) => f.write_fmt(format_args!("{}", i)),
            Object::Real(r) => f.write_fmt(format_args!("{}", r)),
            Object::Name(n) => f.write_fmt(format_args!("/{}", Utf8OrHexDumper(n))),
            Object::String(s, fmt) => match fmt {
                lopdf::StringFormat::Literal => {
                    f.write_char('(')?;
                    f.write_fmt(format_args!("{}", Utf8OrHexDumper(s)))?;
                    f.write_char(')')
                }
                lopdf::StringFormat::Hexadecimal => {
                    f.write_str("<")?;
                    HexDumer(s).fmt(f)?;
                    f.write_str(">")
                }
            },
            Object::Array(a) => {
                f.write_char('[')?;
                for (i, obj) in a.iter().enumerate() {
                    if i > 0 {
                        f.write_char(' ')?;
                    }
                    f.write_fmt(format_args!("{}", ObjectDumper::new(obj)))?;
                }
                f.write_char(']')
            }
            Object::Dictionary(d) => DictionaryDumper::with_indent(d, self.1).fmt(f),
            Object::Stream { .. } => f.write_str("stream"),
            Object::Reference((idx, ver)) => f.write_fmt(format_args!("{} {} R", idx, ver)),
        }
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
        self.1.fmt(f)?;
        f.write_str("<<")?;
        let indent = self.1.inc();
        for (i, (k, v)) in self.0.iter().enumerate() {
            if i > 0 {
                indent.fmt(f)?;
            }
            if !is_complex_pdf_value(v) {
                f.write_fmt(format_args!(
                    "/{} {}\n",
                    Utf8OrHexDumper(k),
                    ObjectDumper::with_indent(v, indent)
                ))?;
            } else {
                f.write_fmt(format_args!("/{}\n", Utf8OrHexDumper(k),))?;
                ObjectDumper::with_indent(v, indent).fmt(f)?;
                f.write_char('\n')?;
            }
        }
        self.1.fmt(f)?;
        f.write_str(">>")
    }
}

/// Return true if `v` is Dictionary or Array
fn is_complex_pdf_value(v: &Object) -> bool {
    match v {
        Object::Dictionary(_) | Object::Array(_) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests;
