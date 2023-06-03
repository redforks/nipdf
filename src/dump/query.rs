use std::borrow::Cow;

use super::object::{ObjectDumper, ObjectIdDumper};
use lopdf::{Dictionary, Document, Object};
use memchr::memmem::Finder;

#[derive(Debug, PartialEq)]
enum FieldQuery<'a> {
    SearchEverywhere(&'a [u8]),
    NameOnly(&'a [u8]),
    NameValueExact(&'a [u8], &'a [u8]),
    NameAndContainsValue(&'a [u8], &'a [u8]),
}

impl<'a> FieldQuery<'a> {
    fn parse(s: &'a str) -> Self {
        if let Some(s) = s.strip_prefix('/') {
            if let Some(pos) = s.find('=') {
                let (name, value) = s.split_at(pos);
                let value = &value[1..];
                if let Some(name) = name.strip_suffix('*') {
                    Self::NameAndContainsValue(name.as_bytes(), value.as_bytes())
                } else {
                    Self::NameValueExact(name.as_bytes(), value.as_bytes())
                }
            } else {
                Self::NameOnly(s.as_bytes())
            }
        } else {
            Self::SearchEverywhere(s.as_bytes())
        }
    }
}

fn as_bytes(value: &Object) -> Cow<[u8]> {
    match value {
        Object::Null => b"Null".as_slice().into(),
        Object::Boolean(b) => if *b { &b"true"[..] } else { &b"false"[..] }.into(),
        Object::Integer(i) => i.to_string().as_bytes().to_vec().into(),
        Object::Real(r) => r.to_string().as_bytes().to_vec().into(),
        Object::Name(n) => n.as_slice().into(),
        Object::String(s, _) => s.as_slice().into(),
        Object::Reference(r) => r.0.to_string().as_bytes().to_vec().into(),
        _ => b"".as_slice().into(),
    }
}

/// Return true if the object matches the given query, value are converted to string before comparison.
fn value_matches(o: &Object, q: &FieldQuery<'_>, ignore_case: bool) -> bool {
    fn bytes_eq(a: &[u8], b: &[u8], ignore_case: bool) -> bool {
        if ignore_case {
            a.eq_ignore_ascii_case(b)
        } else {
            a == b
        }
    }

    fn name_value_matches(n: &[u8], q: &FieldQuery<'_>, ignore_case: bool) -> bool {
        match q {
            FieldQuery::NameOnly(name) => bytes_eq(n, name, ignore_case),
            _ => false,
        }
    }
    fn dict_value_matches(d: &Dictionary, q: &FieldQuery<'_>, ignore_case: bool) -> bool {
        if match q {
            FieldQuery::NameOnly(name) => d.iter().any(|(k, _)| bytes_eq(k, name, ignore_case)),
            FieldQuery::NameValueExact(name, val) => d.iter().any(|(k, v)| {
                bytes_eq(k, name, ignore_case) && bytes_eq(&as_bytes(v), val, ignore_case)
            }),
            FieldQuery::NameAndContainsValue(name, val) => {
                if !ignore_case {
                    let f = Finder::new(val);
                    d.iter().any(|(k, v)| {
                        bytes_eq(k, name, ignore_case) && f.find(&as_bytes(v)).is_some()
                    })
                } else {
                    let val = (*val).to_ascii_lowercase();
                    let f = Finder::new(&val[..]);
                    d.iter().any(|(k, v)| {
                        bytes_eq(k, name, ignore_case)
                            && f.find(&as_bytes(v).to_ascii_lowercase()).is_some()
                    })
                }
            }
            _ => todo!(),
        } {
            true
        } else {
            d.iter()
                .map(|(_, v)| v)
                .any(|v| value_matches(v, q, ignore_case))
        }
    }

    match o {
        Object::Name(n) => name_value_matches(n, q, ignore_case),
        Object::Dictionary(d) => dict_value_matches(d, q, ignore_case),
        Object::Array(a) => a.iter().any(|v| value_matches(v, q, ignore_case)),
        Object::Stream(s) => dict_value_matches(&s.dict, q, ignore_case),
        _ => false,
    }
}

pub fn query(doc: &Document, q: Option<&String>, ignore_case: bool) {
    let field_query = q.map(|s| FieldQuery::parse(s.as_str()));

    doc.objects
        .iter()
        .filter(|(_, o)| {
            if let Some(q) = &field_query {
                if !value_matches(o, q, ignore_case) {
                    return false;
                }
            }
            true
        })
        .for_each(|(id, o)| println!("{}: {}", ObjectIdDumper::new(id), ObjectDumper::new(o)));
}

#[cfg(test)]
mod tests;
