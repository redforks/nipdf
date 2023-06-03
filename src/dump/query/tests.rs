use lopdf::{Stream, StringFormat};

use crate::object::new_name;

use super::*;

#[test]
fn test_filter_object_type() {
    let dict = Dictionary::new();
    assert_eq!(filter_object_type(&Object::Null, ObjectType::Stream), false);
    assert_eq!(
        filter_object_type(&Object::Dictionary(dict.clone()), ObjectType::Stream),
        false
    );
    assert_eq!(
        filter_object_type(
            &Object::Stream(Stream::new(dict, vec![])),
            ObjectType::Stream
        ),
        true
    );
}

#[test]
fn parse_field_query() {
    // name only
    let q = FieldQuery::parse("name");
    assert_eq!(q, FieldQuery::NameOnly(b"name"));

    // Name Value Exact if name=foo
    let q = FieldQuery::parse("name=foo");
    assert_eq!(q, FieldQuery::NameValueExact(b"name", b"foo"));

    // Name and value contains
    let q = FieldQuery::parse("name*=foo");
    assert_eq!(q, FieldQuery::NameAndContainsValue(b"name", b"foo"));
}

#[rstest::rstest]
// name only compares Name, dict name, stream dict name, array children values, dict children values,
// recursively checked, no need to repeat in other FieldQuery cases
#[case(false, Object::Null, "name", false)] // type no name
#[case(true, new_name("name"), "name", false)] // name match
#[case(false, new_name("name"), "Name", false)] // name match case not match
#[case(true, new_name("name"), "Name", true)] // name not match ignore case
#[case(true, Dictionary::from_iter(vec![(b"name".as_slice(), Object::Null)]), "name", false)] // dict name match
#[case(false, Dictionary::new(), "name", false)] // dict name match not match
#[case(true, Dictionary::from_iter(vec![(b"foo".as_slice(), new_name("name"))]), "name", false)] // dict contains value matches
#[case(false, vec![], "name", false)] // array no children
#[case(true, vec![Object::Null, new_name("name")], "name", false)] // array children matches
#[case(false, Stream::new(Dictionary::new(), vec![]), "name", false)] // stream no dict
#[case(true, Stream::new(Dictionary::from_iter(vec![(b"name".as_slice(), Object::Null)]), vec![]), "name", false)] // stream dict name matches
fn test_name_matches(
    #[case] exp: bool,
    #[case] o: impl Into<Object>,
    #[case] q: &str,
    #[case] ignore_case: bool,
) {
    let o = o.into();
    let q = FieldQuery::parse(q);
    assert_eq!(value_matches(&o, &q, ignore_case), exp);
}

#[rstest::rstest]
#[case(false, Object::Null, "=foo", false)]
#[case(true, Object::Null, "=Null", false)]
#[case(false, Object::Null, "=null", false)]
#[case(true, Object::Null, "=null", true)]
#[case(true, Object::Boolean(true), "=true", false)]
#[case(false, Object::Array(vec![]), "=foo", false)]
#[case(true, Object::Boolean(true), "*=tru", false)]
#[case(true, Object::Boolean(true), "*=Tru", true)]
#[case(false, Object::Boolean(true), "*=Tru", false)]
fn test_name_value_matches(
    #[case] exp: bool,
    #[case] o: impl Into<Object>,
    #[case] q: &str,
    #[case] ignore_case: bool,
) {
    let o = o.into();
    let q = format!("name{}", q);
    let q = FieldQuery::parse(&q);

    // name not matches
    let d = Dictionary::from_iter(vec![(b"blah".as_slice(), o.clone())]).into();
    assert_eq!(value_matches(&d, &q, ignore_case), false);

    // name matches checks value
    let d = Dictionary::from_iter(vec![(b"name".as_slice(), o.clone())]).into();
    assert_eq!(value_matches(&d, &q, ignore_case), exp);
}

#[rstest::rstest]
#[case(b"Null", Object::Null)]
#[case(b"true", Object::Boolean(true))]
#[case(b"false", Object::Boolean(false))]
#[case(b"123", Object::Integer(123))]
#[case(b"123.456", Object::Real(123.456))]
#[case(b"foo", Object::String(b"foo".to_vec(), StringFormat::Literal))]
#[case(b"foo", Object::Name(b"foo".to_vec()))]
#[case(b"", vec![])]
#[case(b"", Dictionary::new())]
#[case(b"", Stream::new(Dictionary::new(), vec![]))]
#[case(b"12345", (12345, 2))]
fn test_as_bytes(#[case] exp: &'static [u8], #[case] o: impl Into<Object>) {
    let o = o.into();
    let bytes = as_bytes(&o);
    assert_eq!(bytes, exp);
}
