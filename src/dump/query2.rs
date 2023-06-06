use std::borrow::Cow;

use pdf::primitive::Primitive;

use super::FileWithXRef;

#[derive(Debug, PartialEq)]
enum FieldQuery<'a> {
    SearchEverywhere(&'a str),
    NameOnly(&'a str),
    NameValueExact(&'a str, &'a str),
    NameAndContainsValue(&'a str, &'a str),
}

impl<'a> FieldQuery<'a> {
    fn parse(s: &'a str) -> Self {
        if let Some(s) = s.strip_prefix('/') {
            if let Some(pos) = s.find('=') {
                let (name, value) = s.split_at(pos);
                let value = &value[1..];
                if let Some(name) = name.strip_suffix('*') {
                    Self::NameAndContainsValue(name, value)
                } else {
                    Self::NameValueExact(name, value)
                }
            } else {
                Self::NameOnly(s)
            }
        } else {
            Self::SearchEverywhere(s)
        }
    }
}

fn as_str(v: &Primitive) -> Cow<str> {
    match v {
        Primitive::Null => "null".into(),
        Primitive::Boolean(b) => if *b { "true" } else { "false" }.into(),
        Primitive::Integer(i) => i.to_string().into(),
        Primitive::Number(r) => r.to_string().into(),
        Primitive::Name(n) => n.as_str().into(),
        Primitive::String(s) => s.to_string_lossy().into(),
        Primitive::Reference(r) => r.id.to_string().into(),
        Primitive::Array(_) => "ARRAY".into(),
        Primitive::Dictionary(_) => "DICT".into(),
        Primitive::Stream(_) => "STREAM".into(),
    }
}

/// Return false if no objects match the query.
pub fn query(doc: &FileWithXRef, q: Option<&String>, ignore_case: bool) -> bool {
    todo!()
}

#[cfg(test)]
mod tests;
