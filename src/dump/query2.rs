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

fn search_everywhere_matches(v: &Primitive, s: &str, ignore_case: bool) -> bool {
    todo!()
}

fn search_name_only_matches(v: &Primitive, s: &str, ignore_case: bool) -> bool {
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
                    FieldQuery::SearchEverywhere(s) => search_everywhere_matches(o, s, ignore_case),
                    FieldQuery::NameOnly(s) => search_name_only_matches(o, s, ignore_case),
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
