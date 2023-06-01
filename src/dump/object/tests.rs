use lopdf::StringFormat;

use super::*;

#[test]
fn utf8_or_hex_dumper() {
    let data = b"hello world";
    let dumper = Utf8OrHexDumper(data);
    assert_eq!(format!("{}", dumper), "hello world");

    let data = b"\xf0\x01\x02\x03\x04\x05\x06\x07\x08\t\n\x0b\x0c\r\x0e\x0f";
    let dumper = Utf8OrHexDumper(data);
    assert_eq!(format!("{}", dumper), "0xF00102030405060708090A0B0C0D0E0F");
}

#[test]
fn object_dumper_null() {
    let obj = Object::Null;
    let dumper = ObjectDumper::new(&obj);
    assert_eq!(format!("{}", dumper), "null");
}

#[test]
fn object_dumper_bool() {
    let obj = Object::Boolean(true);
    let dumper = ObjectDumper::new(&obj);
    assert_eq!(format!("{}", dumper), "true");

    let obj = Object::Boolean(false);
    let dumper = ObjectDumper::new(&obj);
    assert_eq!(format!("{}", dumper), "false");
}

#[test]
fn object_dumper_int() {
    let obj = Object::Integer(123);
    let dumper = ObjectDumper::new(&obj);
    assert_eq!(format!("{}", dumper), "123");
}

#[test]
fn object_dumper_real() {
    let obj = Object::Real(123.456);
    let dumper = ObjectDumper::new(&obj);
    assert_eq!(format!("{}", dumper), "123.456");
}

#[test]
fn object_dumper_name() {
    let obj = Object::Name(b"hello".to_vec());
    let dumper = ObjectDumper::new(&obj);
    assert_eq!(format!("{}", dumper), "/hello");
}

#[test]
fn object_dumper_string() {
    let obj = Object::String(b"hello".to_vec(), StringFormat::Literal);
    let dumper = ObjectDumper::new(&obj);
    assert_eq!(format!("{}", dumper), "(hello)");

    let obj = Object::String(b"hello".to_vec(), StringFormat::Hexadecimal);
    let dumper = ObjectDumper::new(&obj);
    assert_eq!(format!("{}", dumper), "<68656C6C6F>");
}

#[test]
fn object_dumper_array() {
    // empty array
    let obj = Object::Array(vec![]);
    let dumper = ObjectDumper::new(&obj);
    assert_eq!(format!("{}", dumper), "[]");

    let obj = Object::Array(vec![
        Object::Null,
        Object::Boolean(true),
        Object::Integer(123),
        Object::Real(123.456),
        Object::Name(b"hello".to_vec()),
        Object::String(b"hello".to_vec(), StringFormat::Literal),
        Object::String(b"hello".to_vec(), StringFormat::Hexadecimal),
    ]);
    let dumper = ObjectDumper::new(&obj);
    assert_eq!(
        format!("{}", dumper),
        "[null true 123 123.456 /hello (hello) <68656C6C6F>]"
    );
}

#[test]
fn object_dumper_dictionary() {
    let obj = Object::Dictionary(Dictionary::new());
    let dumper = ObjectDumper::new(&obj);
    assert_eq!(format!("{}", dumper), "<<>>");
}

#[test]
fn object_dumper_reference() {
    let obj = Object::Reference((123, 456));
    let dumper = ObjectDumper::new(&obj);
    assert_eq!(format!("{}", dumper), "123 456 R");
}

#[test]
fn dictionary_dumper() {
    // empty dictionary
    let obj = Dictionary::new();
    assert_eq!(format!("{}", DictionaryDumper::new(&obj)), "<<>>");

    // one element
    let mut obj = Dictionary::new();
    obj.set("hello", Object::Null);
    assert_eq!(
        format!("{}", DictionaryDumper::new(&obj)),
        r#"
<</hello null
>>
       "#
        .trim()
    );

    // two elements
    let mut obj = Dictionary::new();
    obj.set("hello", Object::Null);
    obj.set("world", Object::Boolean(true));
    assert_eq!(
        format!("{}", DictionaryDumper::new(&obj)),
        r#"
<</hello null
  /world true
>>"#
        .trim()
    );

    // nested dictionary
    let mut obj = Dictionary::new();
    obj.set("hello", Object::Null);
    let mut nested = Dictionary::new();
    nested.set("world", Object::Boolean(true));
    nested.set("hello", Object::Null);
    obj.set("nested", Object::Dictionary(nested));
    assert_eq!(
        format!("{}", DictionaryDumper::new(&obj)),
        r#"
<</hello null
  /nested
  <</world true
    /hello null
  >>
>>"#
        .trim()
    );
}
