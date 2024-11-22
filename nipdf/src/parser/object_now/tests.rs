use super::*;
use prescript::sname;
use test_case::test_case;

#[test_case("null" => Object::Null)]
#[test_case("true" => Object::Bool(true))]
#[test_case("false" => Object::Bool(false))]
#[test_case("123" => Object::Integer(123); "integer")]
#[test_case("-123" => Object::Integer(-123); "negative integer")]
#[test_case("+123" => Object::Integer(123); "integer prefixed with +")]
#[test_case("32488685" => Object::Integer(32488685); "integer can not cast from float")]
#[test_case("4294967296" => Object::Number(4294967296f32); "integer out of range")]
#[test_case("123.12" => Object::Number(123.12); "number")]
#[test_case("-123.12" => Object::Number(-123.12); "negative number")]
#[test_case("-" => Object::Integer(0); "negative symbol only")]
#[test_case("+123.12" => Object::Number(123.12); "number prefixed with +")]
#[test_case("4.0" => Object::Number(4.); "number end with dot")]
#[test_case("4.58984938980.04" => Object::Number(4.58984938980); "number ignore 2nd dot")]
#[test_case("-.002" => Object::Number(-0.002); "number start with dot")]
#[test_case("/" => Object::Name(sname("")); "empty name")]
#[test_case("/foo" => Object::Name(sname("foo")); "name")]
#[test_case("/@foo" => Object::Name(sname("@foo")); "special name")]
#[test_case("/foo#20bar" => Object::Name(sname("foo bar")); "contains hex")]
fn parse_object(buf: &str) -> Object {
    report_parse_err(object().parse(buf.as_bytes()))
}

#[test_case(b"()bar" => (b"bar".as_ref(), "".to_owned()); "empty")]
#[test_case(b"(foo)bar" => (b"bar".as_ref(), "foo".to_owned()); "normal")]
#[test_case(b"(\n)" => (b"".as_ref(), "\n".to_owned()); "contains newline")]
#[test_case(b"(())bar" => (b"bar".as_ref(), "()".to_owned()); "nested empty")]
#[test_case(b"((foo))bar" => (b"bar".as_ref(), "(foo)".to_owned()); "nested")]
#[test_case(b"(foo\\nbar)" => (b"".as_ref(), "foo\nbar".to_owned()); "escaped newline in string")]
#[test_case(b"(foo\\rbar)" => (b"".as_ref(), "foo\rbar".to_owned()); "escaped carriage return in string")]
#[test_case(b"(foo\\tbar)" => (b"".as_ref(), "foo\tbar".to_owned()); "escaped tab in string")]
#[test_case(b"(foo\\bbar)" => (b"".as_ref(), "foo\x08bar".to_owned()); "escaped backspace in string")]
#[test_case(b"(foo\\fbar)" => (b"".as_ref(), "foo\x0Cbar".to_owned()); "escaped form feed in string")]
#[test_case(b"(foo\\(bar)" => (b"".as_ref(), "foo(bar".to_owned()); "escaped left parenthesis in string")]
#[test_case(b"(foo\\)bar)" => (b"".as_ref(), "foo)bar".to_owned()); "escaped right parenthesis in string")]
#[test_case(b"(\\a)" => (b"".as_ref(), "a".to_owned()); "other char escaped to it self")]
#[test_case(b"(\\040)" => (b"".as_ref(), " ".to_owned()); "escaped octal")]
#[test_case(b"(\\7)" => (b"".as_ref(), "\u{7}".to_owned()); "escaped with one octal")]
#[test_case(b"(\\12)" => (b"".as_ref(), "\n".to_owned()); "escaped with two octal")]
#[test_case(b"(\\1414)" => (b"".as_ref(), "a4".to_owned()); "escaped with fourc octal")]
#[test_case(b"(Line1 \\\nLine2 \\\rLine3)" => (b"".as_ref(), "Line1 Line2 Line3".to_owned()); "escaped newline")]
fn test_parse_quoted_string(input: &[u8]) -> (&[u8], String) {
    let (rest, r) = report_peek_err(parse_quoted_string.parse_peek(input));
    (rest, r.as_str().to_string())
}

fn report_parse_err<I, T, E: std::error::Error>(
    rv: Result<T, winnow::error::ParseError<I, E>>,
) -> T {
    rv.map_err(|e| snafu::Report::from_error(e.into_inner()))
        .unwrap()
}

fn report_peek_err<T, E: std::error::Error>(rv: PResult<T, E>) -> T {
    rv.map_err(|e| snafu::Report::from_error(e.into_inner().unwrap()))
        .unwrap()
}
