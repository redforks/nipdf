use super::*;
use crate::file::{ObjectResolver, XRefTable};
use prescript::sname;
use static_assertions::assert_impl_all;
use test_case::test_case;

#[test_case("", b"()"; "empty")]
#[test_case("a", b"(a)"; "single character")]
#[test_case("a(", b"(a\\()"; "left square")]
#[test_case("a)", b"(a\\))"; "right square")]
#[test_case("ab", b"(a\\\nb)"; "escape next \\n")]
#[test_case("ab", b"(a\\\rb)"; "escape next \\r")]
#[test_case("ab", b"(a\\\r\nb)"; "escape next \\r\\n")]
#[test_case("ab", b"(a\\\n\rb)"; "escape next \\n\\r")]
#[test_case("a\nb", b"(a\\\n\nb)"; "escape one next new line")]
#[test_case("a\nb", b"(a\nb)"; "normal new line")]
#[test_case("a\nb", b"(a\rb)"; "normal \\n new line")]
#[test_case("a\nb", b"(a\r\nb)"; "normal \\r\\n new line")]
#[test_case("a\nb", b"(a\n\rb)"; "normal \\n\\r new line")]
#[test_case("\x05a", b"(\\5a)"; "oct 1")]
#[test_case("\x05a", b"(\\05a)"; "oct 2")]
#[test_case("\x05a", b"(\\005a)"; "oct 3")]
fn literal_string_decoded(exp: &str, buf: impl AsRef<[u8]>) {
    assert_eq!(LiteralString::new(buf.as_ref()).as_str(), exp);
}

#[test_case(b"", b"<>" ; "empty")]
#[test_case(b"\x90\x1f\xa3", b"<901FA3>"; "not empty")]
#[test_case(b"\x90\x1f\xa0", b"<901FA>"; "append 0 if odd")]
#[test_case(b"\x90\x1f\xa0", b"<90 1F\tA>"; "ignore whitespace")]
fn hex_string_decoded(exp: impl AsRef<[u8]>, buf: impl AsRef<[u8]>) {
    assert_eq!(HexString::new(buf.as_ref()).as_bytes(), exp.as_ref());
}

#[test_case(Object::LiteralString(LiteralString::new(b"(foo)")), "(foo)"; "literal string")]
#[test_case(Object::HexString(HexString::new(b"<901FA3>")), "<901FA3>"; "hex string")]
#[test_case(Object::Name(sname("foo")), "/foo"; "name")]
fn buf_or_str_to_object(exp: Object, s: &str) {
    assert_eq!(exp, Object::from(s.as_bytes()));
    assert_eq!(exp, Object::from(s));
}

#[test]
fn equal_schema_type_validator() {
    let checker = EqualTypeValueChecker::new(sname("Page"));
    assert!(!checker.check(None));
    assert!(!checker.check(Some(sname("blah"))));
    assert!(checker.check(Some(sname("Page"))));
}

#[test]
fn value_type_validator() {
    let validator = ValueTypeValidator::new(
        NameTypeValueGetter::new(sname("Type")),
        EqualTypeValueChecker::new(sname("Page")) as EqualTypeValueChecker<Name>,
    );
    assert_impl_all!(
        ValueTypeValidator<NameTypeValueGetter, EqualTypeValueChecker<Name>>: TypeValidator
    );

    let mut d = HashMap::new();
    d.insert(sname("a"), "/foo".into());
    let d = Dictionary::from(d);

    assert_eq!(
        Err(ObjectValueError::DictSchemaUnExpectedType(
            "Type: Page".into()
        )),
        validator.valid(&d)
    );
}

#[test]
fn option_value_type_validator() {
    let checker = EqualTypeValueChecker::new(sname("Page")).option();
    assert_impl_all!(OptionTypeValueChecker<EqualTypeValueChecker<Name>>: TypeValueCheck<Name>);

    assert!(checker.check(None));
    assert!(!checker.check(Some(sname("blah"))));
    assert!(checker.check(Some(sname("Page"))));
}

#[test]
fn one_of_type_value_checker() {
    let checker = OneOfTypeValueChecker::new(vec![sname("Page"), sname("Pages")]);
    let schema_type = <OneOfTypeValueChecker<Name> as TypeValueCheck<Name>>::schema_type(&checker);
    assert_eq!("Page|Pages", &schema_type);

    assert!(!checker.check(None::<Name>));
    assert!(!checker.check(Some(sname("blah"))));
    assert!(checker.check(Some(sname("Page"))));
    assert!(checker.check(Some(sname("Pages"))));
}

#[test_case(None => Vec::<u32>::new())]
#[test_case(Some(&[]) => Vec::<u32>::new())]
#[test_case(Some(&[1, 2]) => vec![1, 2])]
fn schema_ref_id_arr(ids: Option<&[u32]>) -> Vec<u32> {
    let mut d = HashMap::new();
    if let Some(ids) = ids {
        let ids: Array = ids.iter().map(|id| Object::new_ref(*id)).collect();
        d.insert(sname("ids"), ids.into());
    }
    let xref = XRefTable::empty();
    let resolver = ObjectResolver::empty(&xref);
    let d = Dictionary::from(d);
    let d = SchemaDict::new(&d, &resolver, ()).unwrap();
    d.ref_id_arr(&sname("ids"))
        .unwrap()
        .into_iter()
        .map(|id| id.get())
        .collect()
}

#[test_case(Object::Null => "null")]
#[test_case(true => "true")]
#[test_case(1i32 => "1")]
#[test_case(1.0f32 => "1.0")]
#[test_case(LiteralString::new(b"(foo)") => "(foo)")]
#[test_case(HexString::new(b"<901FA3>") => "<901fa3>")]
#[test_case("/foo" => "/foo"; "Name")]
#[test_case(vec![] => "[]"; "empty array")]
#[test_case(vec![Object::Null] => "[null]"; "array with null")]
#[test_case(vec![1i32.into(), 2i32.into()] => "[1 2]"; "array with two int")]
#[test_case(15u32 => "15 0 R"; "reference")]
#[test_case(Dictionary::new() => "<<>>"; "empty dict")]
#[test_case([(sname("a"), true.into())].into_iter().collect::<Dictionary>() => "<</a true>>"; "dict with one entry")]
#[test_case([(sname("a"), true.into()), (sname("b"), false.into())].into_iter().collect::<Dictionary>() => "<</a true /b false>>"; "dict with two entries")]
fn pretty_print(o: impl Into<Object>) -> String {
    let o = o.into();
    let s = o.to_doc().pretty(20).to_string();
    s
}

#[test]
fn f32_arr_try_from_object() {
    let arr = vec![1.0f32.into(), 2.0f32.into()];
    let o = Object::Array(arr.into());
    let arr2: [f32; 2] = (&o).try_into().unwrap();
    assert_eq!([1.0f32, 2.0f32], arr2);
}
