use std::{borrow::Cow, cell::OnceCell};

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

fn new_contains(needle: &[u8], ignore_case: bool) -> impl Fn(&[u8]) -> bool + '_ {
    let needle: Cow<[u8]> = if ignore_case {
        needle.to_ascii_lowercase().into()
    } else {
        needle.into()
    };
    #[ouroboros::self_referencing]
    struct F<'a> {
        ignore_case: bool,
        needle: Cow<'a, [u8]>,
        #[borrows(needle)]
        #[covariant]
        f: Finder<'this>,
    }
    let finder = FBuilder {
        needle,
        ignore_case,
        f_builder: |needle| Finder::new(needle),
    }
    .build();
    impl<'a> F<'a> {
        fn contains(&self, haystack: &[u8]) -> bool {
            if *self.borrow_ignore_case() {
                self.borrow_f()
                    .find(&haystack.to_ascii_lowercase())
                    .is_some()
            } else {
                self.borrow_f().find(haystack).is_some()
            }
        }
    }
    move |hay: &[u8]| finder.contains(hay)
}

fn bytes_eq(a: &[u8], b: &[u8], ignore_case: bool) -> bool {
    if ignore_case {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

/// Return true if the object matches the given query, value are converted to string before comparison.
fn value_matches(o: &Object, q: &FieldQuery<'_>, ignore_case: bool) -> bool {
    let contains = OnceCell::new();
    let matches = |o: &Object| {
        let name_value_matches = |n: &[u8]| match q {
            FieldQuery::NameOnly(name) => bytes_eq(n, name, ignore_case),
            FieldQuery::SearchEverywhere(s) => {
                contains.get_or_init(|| new_contains(s, ignore_case))(n)
            }
            _ => false,
        };
        let dict_value_matches = |d: &Dictionary| -> bool {
            if match q {
                FieldQuery::NameOnly(name) => d.iter().any(|(k, _)| bytes_eq(k, name, ignore_case)),
                FieldQuery::NameValueExact(name, val) => d.iter().any(|(k, v)| {
                    bytes_eq(k, name, ignore_case) && bytes_eq(&as_bytes(v), val, ignore_case)
                }),
                FieldQuery::NameAndContainsValue(name, val) => {
                    let f = contains.get_or_init(|| new_contains(val, ignore_case));
                    d.iter()
                        .any(|(k, v)| bytes_eq(k, name, ignore_case) && f(&as_bytes(v)))
                }
                FieldQuery::SearchEverywhere(s) => {
                    let f = contains.get_or_init(|| new_contains(s, ignore_case));
                    d.iter().any(|(k, v)| f(k) || f(&as_bytes(v)))
                }
            } {
                true
            } else {
                d.iter()
                    .map(|(_, v)| v)
                    .any(|v| value_matches(v, q, ignore_case))
            }
        };

        match o {
            Object::Name(n) => name_value_matches(n),
            Object::Dictionary(d) => dict_value_matches(d),
            Object::Array(a) => a.iter().any(|v| value_matches(v, q, ignore_case)),
            Object::Stream(s) => dict_value_matches(&s.dict),
            _ => {
                if let FieldQuery::SearchEverywhere(q) = q {
                    let f = contains.get_or_init(|| new_contains(q, ignore_case));
                    f(&as_bytes(o))
                } else {
                    false
                }
            }
        }
    };
    matches(o)
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
