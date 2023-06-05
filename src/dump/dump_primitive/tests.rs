use super::*;
use crate::object::{new_dictionary1, new_dictionary2, new_pdf_string, new_plain_ref};
use pdf::primitive::{Name, Primitive::Null};
use test_case::test_case;

#[test_case("null", Null)]
#[test_case("true", true)]
#[test_case("false", false)]
#[test_case("123", 123)]
#[test_case("123.456", 123.456)]
#[test_case("(hello)", new_pdf_string(b"hello".to_vec()))]
#[test_case("/hello", Name::from("hello"))]
#[test_case("[]", vec![]; "array")]
#[test_case("<<>>", Dictionary::new(); "dictionary")]
#[test_case("123 456 R", new_plain_ref(123, 456))]
fn primitive_dumper(exp: &str, val: impl Into<Primitive>) {
    assert_eq!(format!("{}", PrimitiveDumper::new(&val.into())), exp);
}

#[test]
fn array_dumper() {
    todo!()
}

#[test]
fn dictionary_dumper() {
    todo!()
}

#[test_case(false, Null)]
#[test_case(false, true)]
#[test_case(false, false)]
#[test_case(false, 123)]
#[test_case(false, 123.456)]
#[test_case(false, new_pdf_string(b"hello".to_vec()))]
#[test_case(true, new_dictionary2("hello", 123, "world", 456))]
#[test_case(false, vec![])]
#[test_case(false, new_plain_ref(123, 456))]
// PdfStream object can not created outside pdf crate, so cannot test it
#[test_case(false, Primitive::Name("hello".into()))]
fn is_complex(exp: bool, val: impl Into<Primitive>) {
    assert_eq!(is_complex_primitive(&val.into()), exp);
}

#[test_case(false, vec![]; "empty array")]
#[test_case(false, vec![Null, Null, Null]; "items less than four")]
#[test_case(true, vec![Null, Null, Null, Null]; "items more than three")]
#[test_case(true, vec![new_dictionary2("hello", 123, "world", 456).into()]; "contains complex item")]
fn test_is_array_complex(exp: bool, val: Vec<Primitive>) {
    assert_eq!(is_array_complex(&val), exp);
}

#[test_case(false, Dictionary::new(); "empty dictionary")]
#[test_case(false, new_dictionary1("hello", 123); "contains one item")]
#[test_case(true, new_dictionary2("hello", 123, "world", 456); "contains two items")]
#[test_case(true, new_dictionary1("foo", new_dictionary2("hello", 123, "bar", 1)); "contains complex item")]
fn test_is_dictionary_complex(exp: bool, val: Dictionary) {
    assert_eq!(is_dictionary_complex(&val), exp);
}
