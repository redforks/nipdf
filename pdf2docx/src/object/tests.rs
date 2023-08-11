use super::*;
use static_assertions::assert_impl_all;
use test_case::test_case;

#[test]
fn test_name_borrow_eq() {
    let borrowed = Name::borrowed(b"foobar");
    let owned = Name::owned(b"foobar".to_vec());
    assert_eq!(
        <Name as Borrow<[u8]>>::borrow(&borrowed),
        <Name as Borrow<[u8]>>::borrow(&owned),
    );

    use std::hash::{Hash, Hasher};
    fn hash<T: Hash>(v: &T) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        let mut hasher = DefaultHasher::new();
        v.hash(&mut hasher);
        hasher.finish()
    }
    assert_eq!(hash(b"foobar"), hash(&borrowed));
    assert_eq!(hash(b"foobar"), hash(&owned));
}

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
    assert_eq!(LiteralString::new(buf.as_ref()).decoded().unwrap(), exp);
}

#[test_case(b"", b"<>" ; "empty")]
#[test_case(b"\x90\x1f\xa3", b"<901FA3>"; "not empty")]
#[test_case(b"\x90\x1f\xa0", b"<901FA>"; "append 0 if odd")]
#[test_case(b"\x90\x1f\xa0", b"<90 1F\tA>"; "ignore whitespace")]
fn hex_string_decoded(exp: impl AsRef<[u8]>, buf: impl AsRef<[u8]>) {
    assert_eq!(
        HexString::new(buf.as_ref()).decoded().unwrap(),
        exp.as_ref()
    );
}

#[test_case(Ok(10), "unknown"; "not exist use default value")]
#[test_case(Ok(1), "a"; "id exist, and is int")]
#[test_case(Err(ObjectValueError::UnexpectedType), "b"; "id exist, but not int")]
fn dict_get_int(exp: Result<i32, ObjectValueError>, id: &str) {
    let mut d = Dictionary::default();
    d.set("a", 1i32);
    d.set("b", "(2)");

    assert_eq!(exp, d.get_int(id, 10));
}

#[test_case(Object::LiteralString("(foo)".into()), "(foo)"; "literal string")]
#[test_case(Object::HexString("<901FA3>".into()), "<901FA3>"; "hex string")]
#[test_case(Object::Name("foo".into()), "/foo"; "name")]
fn buf_or_str_to_object<'a>(exp: Object<'a>, s: &'a str) {
    assert_eq!(exp, Object::from(s.as_bytes()));
    assert_eq!(exp, Object::from(s));
}

#[test]
fn dict_get_bool() {
    let mut d = Dictionary::default();
    d.set("a", true);
    d.set("b", true);
    d.set("c", 1i32);

    assert_eq!(Ok(true), d.get_bool("a", false));
    assert_eq!(Ok(true), d.get_bool("b", true));
    assert_eq!(
        Err(ObjectValueError::UnexpectedType),
        d.get_bool("c", false)
    );
    assert_eq!(Ok(false), d.get_bool("d", false));
}

#[test]
fn dict_get_name_or() {
    let mut d = Dictionary::default();
    d.set("a", "/foo");
    d.set("b", "/bar");
    d.set("c", 1i32);

    assert_eq!(Ok("foo"), d.get_name_or("a", "default"));
    assert_eq!(Ok("bar"), d.get_name_or("b", "default"));
    assert_eq!(
        Err(ObjectValueError::UnexpectedType),
        d.get_name_or("c", "default")
    );
    assert_eq!(Ok("default"), d.get_name_or("d", "default"));
}

#[test]
fn dict_get_name() {
    let mut d = Dictionary::default();
    d.set("a", "/foo");
    d.set("b", "/bar");
    d.set("c", 1i32);

    assert_eq!(Ok(Some("foo")), d.get_name("a"));
    assert_eq!(Ok(Some("bar")), d.get_name("b"));
    assert_eq!(Err(ObjectValueError::UnexpectedType), d.get_name("c"));
    assert_eq!(Ok(None), d.get_name("d"));
}

#[test]
fn equal_schema_type_validator() {
    let checker = EqualTypeValueChecker::str("Page");
    assert_eq!(false, checker.check2(None));
    assert_eq!(false, checker.check2(Some(&"blah")));
    assert_eq!(true, checker.check2(Some(&"Page")));
}

#[test]
fn value_type_validator() {
    let validator = ValueTypeValidator::new(
        NameTypeValueGetter::typ(),
        EqualTypeValueChecker::str("Page") as EqualTypeValueChecker<str, &str>,
    );
    assert_impl_all!(
        ValueTypeValidator<NameTypeValueGetter, EqualTypeValueChecker<str, &str>>: TypeValidator
    );

    let mut d = Dictionary::default();
    d.set("a", "/foo");

    assert_eq!(
        Err(ObjectValueError::DictSchemaUnExpectedType(
            "Type: Page".into()
        )),
        validator.valid2(&d)
    );
}

#[test]
fn option_value_type_validator() {
    let checker = EqualTypeValueChecker::str("Page").option();
    assert_impl_all!(OptionTypeValueChecker<EqualTypeValueChecker<str, &str>>: TypeValueCheck<str>);

    assert!(checker.check2(None));
    assert!(!checker.check2(Some(&"blah")));
    assert!(checker.check2(Some(&"Page")));
}

#[test]
fn str_slice_schema_type_validator() {
    let page_or_pages = ["Pages", "Page"];

    let mut d = Dictionary::new();
    assert_eq!(
        Err(ObjectValueError::DictSchemaError(
            r#"["Pages", "Page"]"#.into(),
            "Type"
        )),
        page_or_pages.valid(&d)
    );

    d.set("Type", 11i32);
    assert_eq!(
        Err(ObjectValueError::DictSchemaError(
            r#"["Pages", "Page"]"#.into(),
            "Type"
        )),
        page_or_pages.valid(&d)
    );

    d.set("Type", "/foo");
    assert_eq!(
        Err(ObjectValueError::DictSchemaUnExpectedType(
            r#"["Pages", "Page"]"#.into(),
        )),
        page_or_pages.valid(&d)
    );

    d.set("Type", "/Pages");
    assert_eq!(Ok(()), page_or_pages.valid(&d));

    d.set("Type", "/Page");
    assert_eq!(Ok(()), page_or_pages.valid(&d));
}

#[test]
fn sub_type_validator() {
    let v = (Some("foo"), "bar");

    let mut d = Dictionary::new();
    assert!(!v.check(&d).unwrap());

    d.set("Subtype", Object::from("/blah"));
    assert!(!v.check(&d).unwrap());

    d.set("Subtype", Object::from("/bar"));
    assert!(v.check(&d).unwrap());

    d.set("Type", Object::from("/X"));
    assert!(!v.check(&d).unwrap());

    d.set("Type", Object::from("/foo"));
    assert!(v.check(&d).unwrap());

    let v = (None, "bar");
    d.set("Type", Object::from(1i32));
    assert!(v.check(&d).unwrap());
}

#[test_case(None => Vec::<u32>::new())]
#[test_case(Some(&[]) => Vec::<u32>::new())]
#[test_case(Some(&[1, 2]) => vec![1, 2])]
fn schema_ref_id_arr(ids: Option<&[u32]>) -> Vec<u32> {
    let mut d = Dictionary::new();
    if let Some(ids) = ids {
        let ids: Array = ids.iter().map(|id| Object::new_ref(*id)).collect();
        d.insert("ids".into(), ids.into());
    }
    let resolver = ObjectResolver::empty();
    let d = SchemaDict::new(&d, &resolver, ()).unwrap();
    d.ref_id_arr("ids")
        .unwrap()
        .into_iter()
        .map(|id| id.get())
        .collect()
}
