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
#[test_case(
    &[Code::Horizontal(Run::new(WHITE, 1), Run::new(BLACK, 2))],
    &[0b001_00011, 0b1_110_0000]
)]
fn test_iter_code(exp: &[Code], buf: &[u8]) {
    let mut next_code = iter_code(buf);
    for e in exp {
        assert_eq!(next_code(WHITE).unwrap().unwrap(), *e);
    }
    assert!(next_code(WHITE).is_none());
    assert!(next_code(WHITE).is_none());
}

#[test_case(WHITE, 0, &[0b0011_0101] ; "white 0")]
#[test_case(WHITE, 1, &[0b0001_1100] ; "white 1")]
#[test_case(WHITE, 64, &[0b1101_1001, 0b1010_1000]; "white 64")]
#[test_case(WHITE, 4005, &[0b0000_0001, 0b1111_0110, 0b1101_1000, 0b1011_0000]; "white 2560+1408+37")]
#[test_case(BLACK, 0, &[0b0000_1101, 0b1100_0000])]
fn test_parse_next_run(color: u8, exp: u16, buf: &[u8]) {
    let mut reader = BitReader::endian(buf, BigEndian);
    let huffman = build_run_huffman();
    assert_eq!(
        Run::new(color, exp),
        next_run(&mut reader, &huffman, color).unwrap()
    );
}
