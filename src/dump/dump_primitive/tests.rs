use super::*;
use crate::object::{new_dictionary1, new_dictionary2, new_pdf_string, new_plain_ref};
use pdf::primitive::{Name, Primitive::Null};
use test_case::test_case;

#[test]
fn utf8_or_hex_dumper() {
    let data = b"hello world";
    let dumper = Utf8OrHexDumper(data);
    assert_eq!(format!("{}", dumper), "hello world");

    let data = b"\xf0\x01\x02\x03\x04\x05\x06\x07\x08\t\n\x0b\x0c\r\x0e\x0f";
    let dumper = Utf8OrHexDumper(data);
    assert_eq!(format!("{}", dumper), "0xF00102030405060708090A0B0C0D0E0F");
}

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

#[test_case("[]", vec![]; "empty")]
#[test_case("[123]", vec![123.into()]; "one")]
#[test_case("[123 456]", vec![123.into(), 456.into()]; "two")]
#[test_case("[
  123
  456
  789
  12
]", vec![123.into(), 456.into(), 789.into(), 12.into()]; "four items use complex format")]
#[test_case("[
  <<>>
  <<>>
  <<>>
  <<>>
]", vec![
    Dictionary::new().into(),
    Dictionary::new().into(),
    Dictionary::new().into(),
    Dictionary::new().into()
]; "nested dict")]
fn array_dumper(exp: &str, items: Vec<Primitive>) {
    assert_eq!(format!("{}", ArrayDumper::new(&items)), exp);
}

#[test_case("<<>>", Dictionary::new(); "empty")]
#[test_case("<</hello 123>>", new_dictionary1("hello", 123); "one simple item")]
#[test_case("<<
  /hello 123
  /world 456
>>", new_dictionary2("hello", 123, "world", 456); "two simple items")]
#[test_case("<<
  /foo
  [
    1
    2
    3
    null
  ]
>>", new_dictionary1("foo", vec![1.into(), 2.into(), 3.into(), Null]); "nested complex array")]
fn dictionary_dumper(exp: &str, d: Dictionary) {
    assert_eq!(format!("{}", DictionaryDumper::new(&d)), exp);
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
