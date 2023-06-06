use std::borrow::Cow;

use super::dump_primitive::{PlainRefDumper, PrimitiveDumper};
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

struct Contains<'a> {
    needle: Cow<'a, str>,
    ignore_case: bool,
}

impl<'a> Contains<'a> {
    fn new(needle: &'a str, ignore_case: bool) -> Self {
        let needle = if ignore_case {
            needle.to_ascii_lowercase().into()
        } else {
            needle.into()
        };
        Self {
            needle,
            ignore_case,
        }
    }

    fn contains(&self, haystack: &str) -> bool {
        if self.ignore_case {
            haystack.to_ascii_lowercase().contains(self.needle.as_ref())
        } else {
            haystack.contains(self.needle.as_ref())
        }
    }
}

/// return true if any `f` call returns true
fn walk_self_and_children(
    v: &Primitive,
    f: &impl Fn(&Primitive) -> bool,
    include_dict_name: bool,
) -> bool {
    if f(v) {
        return true;
    }

    match v {
        Primitive::Array(a) => a
            .iter()
            .any(|v| walk_self_and_children(v, f, include_dict_name)),
        Primitive::Dictionary(d) => {
            if include_dict_name && d.keys().any(|k| f(&Primitive::from(k.clone()))) {
                true
            } else {
                d.values()
                    .any(|v| walk_self_and_children(v, f, include_dict_name))
            }
        }
        Primitive::Stream(s) => s
            .info
            .iter()
            .any(|(_, v)| walk_self_and_children(v, f, include_dict_name)),
        _ => false,
    }
}

fn search_everywhere(v: &Primitive, s: &str, ignore_case: bool) -> bool {
    let contains = Contains::new(s, ignore_case);
    walk_self_and_children(v, &|v| (&contains).contains(as_str(v).as_ref()), true)
}

fn search_name_only(v: &Primitive, s: &str, ignore_case: bool) -> bool {
    todo!()
}

fn search_name_value_exact(v: &Primitive, name: &str, value: &str, ignore_case: bool) -> bool {
    todo!()
}

fn search_name_and_contains_value(
    v: &Primitive,
    name: &str,
    value: &str,
    ignore_case: bool,
) -> bool {
    todo!()
}

/// Return false if no objects match the query.
pub fn query(doc: &FileWithXRef, q: Option<&String>, ignore_case: bool) -> bool {
    let field_query = q.map(|s| FieldQuery::parse(s.as_str()));
    let mut found = false;
    doc.iter_id_object()
        .filter(|(_, o)| {
            if let Some(field_query) = &field_query {
                match field_query {
                    FieldQuery::SearchEverywhere(s) => search_everywhere(o, s, ignore_case),
                    FieldQuery::NameOnly(s) => search_name_only(o, s, ignore_case),
                    FieldQuery::NameValueExact(name, value) => {
                        search_name_value_exact(o, name, value, ignore_case)
                    }
                    FieldQuery::NameAndContainsValue(name, value) => {
                        search_name_and_contains_value(o, name, value, ignore_case)
                    }
                }
            } else {
                true
            }
        })
        .for_each(|(id, o)| {
            found = true;
            println!("{}: {}", PlainRefDumper(&id), PrimitiveDumper::new(&o));
        });
    found
}

#[cfg(test)]
mod tests;
