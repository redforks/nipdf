use lopdf::{Dictionary, Stream};

use super::*;

#[test]
fn indent_display() {
    assert_eq!(format!("{}", Indent(0)), "");
    assert_eq!(format!("{}", Indent(1)), "  ");
    assert_eq!(format!("{}", Indent(2)), "    ");
}

#[test]
fn indent_inc() {
    assert_eq!(Indent(0).inc(), Indent(1));
    assert_eq!(Indent(1).inc(), Indent(2));
    assert_eq!(Indent(2).inc(), Indent(3));
}

#[test]
fn from_object_to_object_type() {
    assert_eq!(ObjectType::from(&Object::Null), ObjectType::Other);
    assert_eq!(ObjectType::from(&Object::Boolean(true)), ObjectType::Other);
    assert_eq!(ObjectType::from(&Object::Integer(1)), ObjectType::Other);
    assert_eq!(ObjectType::from(&Object::Real(1.0)), ObjectType::Other);
    assert_eq!(
        ObjectType::from(&Object::Name("".into())),
        ObjectType::Other
    );
    assert_eq!(ObjectType::from(&Object::Array(vec![])), ObjectType::Other);
    assert_eq!(
        ObjectType::from(&Object::Dictionary(Dictionary::new())),
        ObjectType::Other
    );
    assert_eq!(
        ObjectType::from(&Object::Stream(Stream::new(Dictionary::new(), vec![]))),
        ObjectType::Stream
    );
    assert_eq!(
        ObjectType::from(&Object::Reference((1, 1))),
        ObjectType::Other
    );
}
