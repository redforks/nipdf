use super::*;
use crate::object::{new_dictionary1, new_pdf_string, new_plain_ref};
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

#[test_case(true, "foo", "foo", false; "exact match")]
#[test_case(true, "Foo", "fOo", true; "exact match, ignore case")]
#[test_case(true, "foo", "foobar", false; "contains")]
#[test_case(true, "Foo", "fOoBar", true; "contains, ignore case")]
#[test_case(false, "foo", "bar", false; "no match")]
#[test_case(false, "foobar", "foo", true; "no contains")]
fn test_contains(exp: bool, needle: &str, haystack: &str, ignore_case: bool) {
    let c = Contains::new(needle, ignore_case);
    assert_eq!(c.contains(haystack), exp);
}

#[test_case(true, Primitive::Null, "null")]
#[test_case(false, Primitive::Null, "blah")]
#[test_case(true, new_dictionary1("foo", 1), "foo")]
#[test_case(true, vec![Dictionary::new().into()], "DIC")]
#[test_case(true, new_dictionary1("blah", new_pdf_string("foo")), "foo")]
#[test_case(true, new_dictionary1("blah", vec![new_pdf_string("foo").into()]), "foo")]
fn test_search_everywhere(exp: bool, v: impl Into<Primitive>, s: &str) {
    assert_eq!(search_everywhere_matches(&v.into(), s, false), exp);
}
