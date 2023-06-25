use super::*;
use test_case::test_case;

#[test]
fn test_decode() {
    insta::assert_debug_snapshot!(decode(include_bytes!("./ccitt-1"), 24, None).unwrap());
}

#[test_case(&[], &[]; "empty")]
#[test_case(&[Code::Pass], &[0b0001_0000]; "pass")]
#[test_case(&[Code::Vertical(0)], &[0b1000_0000])]
#[test_case(&[Code::Vertical(1)], &[0b0110_0000])]
#[test_case(&[Code::Vertical(2)], &[0b0000_1100])]
#[test_case(&[Code::Vertical(-1)], &[0b0100_0000])]
#[test_case(&[Code::Vertical(-2)], &[0b0000_1000])]
#[test_case(&[Code::Vertical(-3)], &[0b0000_0100])]
#[test_case(&[Code::Vertical(0), Code::Vertical(0)], &[0b1100_0000])]
fn test_iter_code(exp: &[Code], buf: &[u8]) {
    let mut iter = iter_code(buf);
    for e in exp {
        assert_eq!(iter.next().unwrap().unwrap(), *e);
    }
    assert!(iter.next().is_none());
    assert!(iter.next().is_none());
}
