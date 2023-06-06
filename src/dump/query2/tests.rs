use super::*;
use test_case::test_case;

#[test_case(FieldQuery::NameOnly("name"), "/name"; "name only")]
#[test_case(FieldQuery::NameValueExact("name", "foo"), "/name=foo"; "name value exact")]
#[test_case(FieldQuery::NameAndContainsValue("name", "foo"), "/name*=foo"; "name and contains value")]
#[test_case(FieldQuery::SearchEverywhere("foo"), "foo"; "search everywhere")]
fn parse_field_query(exp: FieldQuery, s: &str) {
    let q = FieldQuery::parse(s);
    assert_eq!(q, exp);
}
