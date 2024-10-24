use super::*;
use prescript::sname;

#[test]
fn try_from_object_encoding_differences() {
    // empty
    let obj = Object::Array(vec![].into());
    let res = EncodingDifferences::try_from(&obj).unwrap();
    assert!(res.0.is_empty());

    // normal
    let obj = Object::Array(
        vec![
            Object::Integer(1),
            Object::Name(sname("A")),
            Object::Integer(3),
            Object::Name(sname("B")),
            Object::Name(sname("C")),
        ]
        .into(),
    );
    let res = EncodingDifferences::try_from(&obj).unwrap();
    assert_eq!(res.0.len(), 3);
    assert_eq!(res.0[&1], "A");
    assert_eq!(res.0[&3], "B");
    assert_eq!(res.0[&4], "C");
}
