use lopdf::{Stream, StringFormat};
use test_case::test_case;

use crate::object::new_name;

use super::*;

#[test]
fn parse_field_query() {
    // name only
    let q = FieldQuery::parse("/name");
    assert_eq!(q, FieldQuery::NameOnly(b"name"));

    // Name Value Exact if name=foo
    let q = FieldQuery::parse("/name=foo");
    assert_eq!(q, FieldQuery::NameValueExact(b"name", b"foo"));

    // Name and value contains
    let q = FieldQuery::parse("/name*=foo");
    assert_eq!(q, FieldQuery::NameAndContainsValue(b"name", b"foo"));

    // search everywhere
    let q = FieldQuery::parse("foo");
    assert_eq!(q, FieldQuery::SearchEverywhere(b"foo"));
}

// name only compares Name, dict name, stream dict name, array children values, dict children values,
// recursively checked, no need to repeat in other FieldQuery cases
#[test_case(false, Object::Null, "name", false; "type no name")]
#[test_case(true, new_name("name"), "name", false; "name matches")]
#[test_case(false, new_name("name"), "Name", false; "name match case not match")]
#[test_case(true, new_name("name"), "Name", true; "name not match ignore case")]
#[test_case(true, Dictionary::from_iter(vec![(b"name".as_slice(), Object::Null)]), "name", false; "dict name match")]
#[test_case(false, Dictionary::new(), "name", false; "dict name match not match")]
#[test_case(true, Dictionary::from_iter(vec![(b"foo".as_slice(), new_name("name"))]), "name", false; "dict contains value matches")]
#[test_case(false, vec![], "name", false; "array no children")]
#[test_case(true, vec![Object::Null, new_name("name")], "name", false; "array children matches")]
#[test_case(false, Stream::new(Dictionary::new(), vec![]), "name", false; "stream no dict")]
#[test_case(true, Stream::new(Dictionary::from_iter(vec![(b"name".as_slice(), Object::Null)]), vec![]), "name", false; "stream dict name matches")]
fn test_name_matches(exp: bool, o: impl Into<Object>, q: &str, ignore_case: bool) {
    let o = o.into();
    let q = format!("/{}", q);
    let q = FieldQuery::parse(&q);
    assert_eq!(value_matches(&o, &q, ignore_case), exp);
}

#[test_case(false, Object::Null, "=foo", false)]
#[test_case(true, Object::Null, "=Null", false)]
#[test_case(false, Object::Null, "=null", false)]
#[test_case(true, Object::Null, "=null", true)]
#[test_case(true, Object::Boolean(true), "=true", false)]
#[test_case(false, Object::Array(vec![]), "=foo", false)]
#[test_case(true, Object::Boolean(true), "*=tru", false)]
#[test_case(true, Object::Boolean(true), "*=Tru", true)]
#[test_case(false, Object::Boolean(true), "*=Tru", false)]
fn test_name_value_matches(exp: bool, o: impl Into<Object>, q: &str, ignore_case: bool) {
    let o = o.into();
    let q = format!("/name{}", q);
    let q = FieldQuery::parse(&q);

    // name not matches
    let d = Dictionary::from_iter(vec![(b"blah".as_slice(), o.clone())]).into();
    assert!(!value_matches(&d, &q, ignore_case));

    // name matches checks value
    let d = Dictionary::from_iter(vec![(b"name".as_slice(), o)]).into();
    assert_eq!(value_matches(&d, &q, ignore_case), exp);
}

#[test_case(true, Object::Null, "Null", false)]
#[test_case(false, Object::Null, "null", false)]
#[test_case(true, Object::Null, "Null", true)]
#[test_case(true, Object::Null, "nU", true)]
#[test_case(true, new_name("Name"), "naM", true)]
#[test_case(true, Dictionary::from_iter([(b"name".as_slice(), Object::Null)]), "name", false)]
#[test_case(false, Dictionary::from_iter([(b"name".as_slice(), Object::Null)]), "NAme", false)]
#[test_case(true, Dictionary::from_iter([(b"name".as_slice(), Object::Null)]), "NAme", true)]
#[test_case(true, Dictionary::from_iter([(b"foo".as_slice(), new_name("nAme"))]), "NAme", true)]
#[test_case(true, Dictionary::from_iter([(b"foo".as_slice(), vec![Object::Null].into())]), "null", true)]
fn test_search_everywhere(exp: bool, o: impl Into<Object>, q: &str, ignore_case: bool) {
    let o = o.into();
    let q = FieldQuery::parse(q);
    assert_eq!(value_matches(&o, &q, ignore_case), exp);
}

#[test_case(b"Null", Object::Null)]
#[test_case(b"true", Object::Boolean(true))]
#[test_case(b"false", Object::Boolean(false))]
#[test_case(b"123", Object::Integer(123))]
#[test_case(b"123.456", Object::Real(123.456))]
#[test_case(b"foo", Object::String(b"foo".to_vec(), StringFormat::Literal))]
#[test_case(b"foo", Object::Name(b"foo".to_vec()))]
#[test_case(b"", vec![])]
#[test_case(b"", Dictionary::new())]
#[test_case(b"", Stream::new(Dictionary::new(), vec![]))]
#[test_case(b"12345", (12345, 2))]
fn test_as_bytes(exp: &'static [u8], o: impl Into<Object>) {
    let o = o.into();
    let bytes = as_bytes(&o);
    assert_eq!(bytes, exp);
}
