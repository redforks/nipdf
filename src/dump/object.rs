//! Function and Types to dump `lopdf::Object` enum values
use std::fmt::Display;
use std::fmt::Write;
use std::str::from_utf8;

use lopdf::Dictionary;
use lopdf::Object;

/// Dump `[u8]` as utf8 str, or hex if not valid utf8
pub struct Utf8OrHexDumper<'a>(pub &'a [u8]);

impl<'a> Display for Utf8OrHexDumper<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match from_utf8(self.0) {
            Ok(s) => f.write_fmt(format_args!("\"{}\"", s)),
            Err(_) => f.write_fmt(format_args!("0x{}", &hex::encode_upper(self.0))),
        }
    }
}

pub struct ObjectDumper<'a>(pub &'a Object);

impl<'a> Display for ObjectDumper<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            Object::Null => f.write_str("[null]"),
            Object::Boolean(b) => f.write_str(if *b { "true" } else { "false" }),
            Object::Integer(i) => f.write_fmt(format_args!("{}", i)),
            Object::Real(r) => f.write_fmt(format_args!("{}", r)),
            Object::Name(n) => f.write_fmt(format_args!("/{}", Utf8OrHexDumper(n))),
            Object::String(s, fmt) => {
                f.write_fmt(format_args!("[{:?}] ({})", fmt, Utf8OrHexDumper(s)))
            }
            Object::Array(a) => {
                f.write_char('[')?;
                for (i, obj) in a.iter().enumerate() {
                    if i > 0 {
                        f.write_char(' ')?;
                    }
                    f.write_fmt(format_args!("{}", ObjectDumper(obj)))?;
                }
                f.write_char(']')
            }
            Object::Dictionary(d) => DictionaryDumper(d).fmt(f),
            Object::Stream { .. } => f.write_str("stream"),
            Object::Reference { .. } => f.write_str("reference"),
        }
    }
}

pub struct DictionaryDumper<'a>(pub &'a Dictionary);

impl<'a> Display for DictionaryDumper<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_char('<')?;
        for (i, (k, v)) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_char(' ')?;
            }
            f.write_fmt(format_args!("/{} {}", Utf8OrHexDumper(k), ObjectDumper(v)))?;
        }
        f.write_char('>')
    }
}
