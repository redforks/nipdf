use crate::{
    object::new_name,
    stream::{FILTER_ZERO_DECODER, KEY_DECODE_PARMS, KEY_FILTER},
};
use lopdf::{Dictionary as Dict, Object, Object::*, Stream, StringFormat};

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
    let obj = Object::Array(vec![]);
    let dumper = ObjectDumper::new(&obj);
    assert_eq!(format!("{}", dumper), "[]");
}

#[test]
fn object_dumper_dictionary() {
    let obj = Object::Dictionary(Dict::new());
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
fn object_dumper_stream() {
    let obj = Object::Stream(Stream::new(Dict::new(), b"hello".to_vec()));
    assert_eq!(
        format!("{}", ObjectDumper::new(&obj)),
        "STREAM allows_compression\n  <</Length 5>>"
    );
}

#[test]
fn dictionary_dumper() {
    // empty dictionary
    let obj = Dict::new();
    assert_eq!(format!("{}", DictionaryDumper::new(&obj)), "<<>>");

    // one element
    let mut obj = Dict::new();
    obj.set("hello", Object::Null);
    assert_eq!(
        format!("{}", DictionaryDumper::new(&obj)),
        "<</hello null>>"
    );

    // two elements
    let mut obj = Dict::new();
    obj.set("hello", Object::Null);
    obj.set("world", Object::Boolean(true));
    assert_eq!(
        format!("{}", DictionaryDumper::new(&obj)),
        r#"
<<
  /hello null
  /world true
>>"#
        .trim()
    );

    // nested dictionary
    let mut obj = Dict::new();
    obj.set("hello", Object::Null);
    let mut nested = Dict::new();
    nested.set("world", Object::Boolean(true));
    nested.set("hello", Object::Null);
    obj.set("nested", Object::Dictionary(nested));
    assert_eq!(
        format!("{}", DictionaryDumper::new(&obj)),
        r#"
<<
  /hello null
  /nested
  <<
    /world true
    /hello null
  >>
>>"#
        .trim()
    );
}

#[test]
fn array_dumper() {
    // empty array
    let obj = vec![];
    assert_eq!(format!("{}", ArrayDumper::new(&obj)), "[]");

    // one element
    let obj = vec![Null];
    assert_eq!(format!("{}", ArrayDumper::new(&obj)), "[null]");

    // two elements
    let obj = vec![Null, Boolean(true)];
    assert_eq!(format!("{}", ArrayDumper::new(&obj)), "[null true]");

    // nested if more than 3 elements
    let obj = vec![Null, Boolean(true), Null, Integer(34)];
    assert_eq!(
        format!("{}", ArrayDumper::new(&obj)),
        r#"
[
  null
  true
  null
  34
]"#
        .trim()
    );

    // nested array
    let obj = vec![
        Null,
        Array(vec![Boolean(true), Null]),
        Array(vec![Boolean(true), Null, Integer(65), Real(12.34)]),
    ];
    assert_eq!(
        format!("{}", ArrayDumper::new(&obj)),
        r#"
[
  null
  [true null]
  [
    true
    null
    65
    12.34
  ]
]"#
        .trim()
    );
}

#[test]
fn test_is_complex_pdf_value() {
    // non array/dictionary types are simle
    assert!(!is_complex_pdf_value(&Null));
    assert!(!is_complex_pdf_value(&Boolean(true)));
    assert!(!is_complex_pdf_value(&Integer(123)));
    assert!(!is_complex_pdf_value(&Real(123.456)));
    assert!(!is_complex_pdf_value(&Name(b"hello".to_vec())));
    assert!(!is_complex_pdf_value(&String(
        b"hello".to_vec(),
        StringFormat::Literal
    )));
    assert!(!is_complex_pdf_value(&String(
        b"hello".to_vec(),
        StringFormat::Hexadecimal
    )));
    assert!(!is_complex_pdf_value(&Reference((123, 456))));

    // array items less than 4, and do not contains complex types are simple
    let empty_arr = Array(vec![]);
    let four_items_arr = Array(vec![Null, Null, Null, Null]);
    assert!(!is_complex_pdf_value(&empty_arr));
    assert!(!is_complex_pdf_value(&Array(vec![
        Null,
        Boolean(true),
        Integer(123),
    ])));
    assert!(is_complex_pdf_value(&four_items_arr));
    assert!(!is_complex_pdf_value(&Array(vec![empty_arr])));
    assert!(is_complex_pdf_value(&Array(vec![four_items_arr.clone()])));

    // Dictionary items less than 2, and do not contains complex types are simple
    let empty_dict = Dict::new();
    assert!(!is_complex_pdf_value(&Dictionary(empty_dict)));
    // not complex if one simple item
    assert!(!is_complex_pdf_value(&Dictionary(
        vec![(b"hello".to_vec(), Null)].into_iter().collect()
    )));
    // complex if more than one items
    assert!(is_dictionary_complex(
        &vec![
            (b"hello".to_vec(), Null),
            (b"world".to_vec(), Boolean(true))
        ]
        .into_iter()
        .collect()
    ));
    // complex if item value is complex
    assert!(is_dictionary_complex(
        &vec![(b"hello".to_vec(), four_items_arr)]
            .into_iter()
            .collect()
    ));
}

#[test]
fn object_id_dumper() {
    let obj = (123, 456);
    assert_eq!(format!("{}", ObjectIdDumper::new(&obj)), "123 456 R");
}

#[test]
fn stream_dumper_test() {
    // allows compression, with start_position
    let dict = Dict::from_iter([("hello", Object::Null), ("world", Object::Null)]);
    let stream = Stream::with_position(dict, 1234).with_compression(true);
    assert_eq!(
        format!("{}", StreamDumper::new(&stream)),
        r#"
STREAM allows_compression @1234
  <<
    /hello null
    /world null
  >>
"#
        .trim()
    );

    // not allow compression, without start_position
    let dict = Dict::new();
    let stream = Stream::new(dict, vec![]).with_compression(false);
    assert_eq!(
        format!("{}", StreamDumper::new(&stream)),
        r#"
STREAM not allows_compression
  <</Length 0>>
"#
        .trim()
    );

    // indent
    let dict = Dict::from_iter([("hello", Object::Null), ("world", Object::Null)]);
    let stream = Stream::new(dict, vec![]).with_compression(false);
    assert_eq!(
        format!("{}", StreamDumper::with_indent(&stream, Indent(1))),
        r#"
  STREAM not allows_compression
    <<
      /hello null
      /world null
      /Length 0
    >>
"#
        .trim_matches('\n')
    );
}

#[test]
fn dump_stream_content() {
    let stream = Stream::new(
        [(KEY_FILTER, new_name(FILTER_ZERO_DECODER))]
            .into_iter()
            .collect(),
        vec![1, 2, 3, 4, 5],
    );

    // dump content without decode
    let mut dest = vec![];
    let result = StreamContentDumper::new(&stream, false).write_content(&mut dest);
    assert_eq!(result.unwrap(), 5);
    assert_eq!(dest, vec![1, 2, 3, 4, 5]);

    // dump decoded content
    let mut dest = vec![];
    let result = StreamContentDumper::new(&stream, true).write_content(&mut dest);
    assert_eq!(result.unwrap(), 5);
}
