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

fn bytes_eq(a: &[u8], b: &[u8], ignore_case: bool) -> bool {
    if ignore_case {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

fn iter_self_and_children(o: &Object) -> impl Iterator<Item = &Object> {
    let mut stack = vec![o];
    std::iter::from_fn(move || {
        if let Some(o) = stack.pop() {
            match o {
                Object::Dictionary(d) => {
                    stack.extend(d.iter().map(|(_, v)| v));
                }
                Object::Array(a) => {
                    stack.extend(a.iter());
                }
                Object::Stream(stream) => {
                    stack.extend(stream.dict.iter().map(|(_, v)| v));
                }
                _ => {}
            }
            Some(o)
        } else {
            None
        }
    })
}

fn search_everywhere_matches(o: &Object, s: &[u8], ignore_case: bool) -> bool {
    let lower_s;
    let f = if ignore_case {
        lower_s = s.to_ascii_lowercase();
        Finder::new(&lower_s)
    } else {
        Finder::new(s)
    };

    fn contains(v: &[u8], f: &Finder, ignore_case: bool) -> bool {
        if ignore_case {
            f.find(&v.to_ascii_lowercase()).is_some()
        } else {
            f.find(v).is_some()
        }
    }

    fn inner(o: &Object, f: &Finder, ignore_case: bool) -> bool {
        match o {
            Object::Dictionary(d) => d.iter().any(|(k, _)| contains(k, f, ignore_case)),
            _ => contains(&as_bytes(o), f, ignore_case),
        }
    }

    iter_self_and_children(o).any(|o| inner(o, &f, ignore_case))
}

fn name_only(o: &Object, name: &[u8], ignore_case: bool) -> bool {
    fn dict(o: &Dictionary, name: &[u8], ignore_case: bool) -> bool {
        o.iter().any(|(k, _)| bytes_eq(k, name, ignore_case))
    }

    iter_self_and_children(o).any(|o| match o {
        Object::Name(n) => bytes_eq(n, name, ignore_case),
        Object::Dictionary(d) => dict(d, name, ignore_case),
        Object::Stream(stream) => dict(&stream.dict, name, ignore_case),
        _ => false,
    })
}

fn name_value_exact(o: &Object, name: &[u8], value: &[u8], ignore_case: bool) -> bool {
    fn dict(o: &Dictionary, name: &[u8], value: &[u8], ignore_case: bool) -> bool {
        o.iter().any(|(k, v)| {
            bytes_eq(k, name, ignore_case) && bytes_eq(&as_bytes(v), value, ignore_case)
        })
    }

    match o {
        Object::Dictionary(d) => dict(d, name, value, ignore_case),
        Object::Stream(stream) => dict(&stream.dict, name, value, ignore_case),
        _ => false,
    }
}

fn name_and_contains_value(o: &Object, name: &[u8], value: &[u8], ignore_case: bool) -> bool {
    let lower_s;
    let f = if ignore_case {
        lower_s = value.to_ascii_lowercase();
        Finder::new(&lower_s)
    } else {
        Finder::new(value)
    };

    fn contains(v: &[u8], f: &Finder, ignore_case: bool) -> bool {
        if ignore_case {
            f.find(&v.to_ascii_lowercase()).is_some()
        } else {
            f.find(v).is_some()
        }
    }

    fn dict(o: &Dictionary, name: &[u8], f: &Finder, ignore_case: bool) -> bool {
        o.iter()
            .any(|(k, v)| bytes_eq(k, name, ignore_case) && contains(&as_bytes(v), f, ignore_case))
    }

    match o {
        Object::Dictionary(d) => dict(d, name, &f, ignore_case),
        Object::Stream(stream) => dict(&stream.dict, value, &f, ignore_case),
        _ => false,
    }
}

/// Return true if the object matches the given query, value are converted to string before comparison.
fn value_matches(o: &Object, q: &FieldQuery<'_>, ignore_case: bool) -> bool {
    match q {
        FieldQuery::SearchEverywhere(s) => search_everywhere_matches(o, s, ignore_case),
        FieldQuery::NameOnly(name) => name_only(o, name, ignore_case),
        FieldQuery::NameValueExact(name, val) => name_value_exact(o, name, val, ignore_case),
        FieldQuery::NameAndContainsValue(name, val) => {
            name_and_contains_value(o, name, val, ignore_case)
        }
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
