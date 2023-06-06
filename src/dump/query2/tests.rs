use super::*;
use crate::object::{new_name, new_pdf_string, new_plain_ref};
use istring::small::SmallString;
use pdf::primitive::{Dictionary, Name, Primitive};
use test_case::test_case;

#[test_case(FieldQuery::NameOnly("name"), "/name"; "name only")]
#[test_case(FieldQuery::NameValueExact("name", "foo"), "/name=foo"; "name value exact")]
#[test_case(FieldQuery::NameAndContainsValue("name", "foo"), "/name*=foo"; "name and contains value")]
#[test_case(FieldQuery::SearchEverywhere("foo"), "foo"; "search everywhere")]
fn parse_field_query(exp: FieldQuery, s: &str) {
    let q = FieldQuery::parse(s);
    assert_eq!(q, exp);
}

#[test_case("null", Primitive::Null; "null value")]
#[test_case("15", 15; "int")]
#[test_case("1.15", 1.15; "number")]
#[test_case("true", true; "bool: true")]
#[test_case("false", false; "bool: false")]
#[test_case("foo", new_pdf_string("foo"); "string")]
#[test_case("33", new_plain_ref(33, 1); "reference")]
#[test_case("ARRAY", vec![]; "array")]
#[test_case("DICT", Dictionary::new(); "dict")]
#[test_case("name", Name(SmallString::from("name")); "name")]
fn test_as_str(exp: &str, v: impl Into<Primitive>) {
    assert_eq!(as_str(&v.into()), Cow::from(exp));
}
