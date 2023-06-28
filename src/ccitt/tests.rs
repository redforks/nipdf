use super::*;
use test_case::test_case;

#[test_log::test]
fn test_decode() {
    let flags = Flags {
        encoded_byte_align: true,
        ..Default::default()
    };
    // ccitt-1 extracted by `dump-pdf stream -f sample_files/normal/pdfreference1.0.pdf 643 --raw`
    insta::assert_debug_snapshot!(decode(include_bytes!("./ccitt-1"), 24, None, flags).unwrap());
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
    &[0b001_00011, 0b1110_0000]
)]
#[test_case(
    &[Code::EndOfFassimileBlock],
    &[0b0, 0b0001_0000, 0b0000_0001]
)]
fn test_iter_code(exp: &[Code], buf: &[u8]) {
    let flags = Flags::default();
    let mut next_code = iter_code(buf);
    let last_buf = repeat(0).take(4).collect::<Vec<_>>();
    let mut cur_buf = repeat(0).take(4).collect::<Vec<_>>();
    let mut coder = Coder::new(&last_buf, &mut cur_buf);
    coder.pos = Some(0); // disable new_line flag
    for e in exp {
        assert_eq!(next_code(&mut coder, &flags).unwrap().unwrap(), *e);
    }
    assert!(next_code(&mut coder, &flags).is_none());
    assert!(next_code(&mut coder, &flags).is_none());
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
