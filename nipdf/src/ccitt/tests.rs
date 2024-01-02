use super::*;
use test_case::test_case;

#[test_log::test]
fn test_decode_group4() {
    let flags = Flags {
        encoded_byte_align: true,
        ..Default::default()
    };
    let decoder = Decoder {
        algorithm: Algorithm::Group4,
        flags,
        width: 24,
        rows: None,
    };

    // ccitt-1 extracted by `dump-pdf stream -f sample_files/normal/pdfreference1.0.pdf 643 --raw`
    insta::assert_debug_snapshot!(decoder.decode(include_bytes!("./ccitt-1")).unwrap());
}

#[test]
fn group4_inverse_black_white() {
    let flags = Flags {
        encoded_byte_align: true,
        inverse_black_white: true,
        ..Default::default()
    };
    let decoder = Decoder {
        algorithm: Algorithm::Group4,
        flags,
        width: 24,
        rows: None,
    };
    // ccitt-1 extracted by `dump-pdf stream -f sample_files/normal/pdfreference1.0.pdf 643 --raw`
    insta::assert_debug_snapshot!(decoder.decode(include_bytes!("./ccitt-1")).unwrap());
}

#[test]
fn group4_false_end_of_block() {
    let flags = Flags {
        end_of_block: false,
        ..Default::default()
    };
    let decoder = Decoder {
        algorithm: Algorithm::Group4,
        flags,
        width: 81,
        rows: Some(26),
    };
    // extracted by `dump-pdf stream -f pdf.js/test/pdfs/ccitt_EndOfBlock_false.pdf 6 --raw`
    insta::assert_debug_snapshot!(
        decoder
            .decode(include_bytes!("ccitt-false-end-of-block"))
            .unwrap()
    );
}

// #[test]
// fn group3_1d() {
//     let flags = Flags {
//         end_of_block: false,
//         ..Default::default()
//     };
//     let decoder = Decoder {
//         algorithm: Algorithm::Group3_1D,
//         flags,
//         width: 81,
//         rows: Some(26),
//     };
//     // extracted by `dump-pdf stream -f pdf.js/test/pdfs/ccitt_EndOfBlock_false.pdf 8 --raw`
//     insta::assert_debug_snapshot!(decoder.decode(include_bytes!("ccitt-group3-1D")).unwrap());
// }

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
    &[Code::Horizontal(PictualElement::from_color(Color::White, 1), PictualElement::from_color(Color::Black, 2))],
    &[0b001_00011, 0b1110_0000]
)]
#[test_case(
    &[Code::EndOfFassimileBlock],
    &[0b0, 0b0001_0000, 0b0000_0001]
)]
fn test_iter_code_group4(exp: &[Code], buf: &[u8]) {
    let flags = Flags::default();
    let mut next_code = iter_code(Algorithm::Group4, buf);
    let state = State::default();
    for e in exp {
        assert_eq!(next_code(state, &flags).unwrap().unwrap(), *e);
    }
    assert!(next_code(state, &flags).is_none());
    assert!(next_code(state, &flags).is_none());
}

#[test_case(Color::White, 0, &[0b0011_0101] ; "white 0")]
#[test_case(Color::White, 1, &[0b0001_1100] ; "white 1")]
#[test_case(Color::White, 64, &[0b1101_1001, 0b1010_1000]; "white 64")]
#[test_case(Color::White, 4005, &[0b0000_0001, 0b1111_0110, 0b1101_1000, 0b1011_0000]; "white 2560+1408+37")]
#[test_case(Color::Black, 0, &[0b0000_1101, 0b1100_0000])]
fn test_parse_next_run(color: Color, exp: u16, buf: &[u8]) {
    let mut reader = BitReader::endian(buf, BigEndian);
    let huffman = build_run_huffman(Algorithm::Group4);
    assert_eq!(
        PictualElement::from_color(color, exp),
        next_run(&mut reader, &huffman, color).unwrap()
    );
}
