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
    let data = decoder
        .decode(include_bytes!("ccitt-false-end-of-block"))
        .unwrap();
    assert_eq!((81 * 26 + 7) / 8, data.len());
    insta::assert_debug_snapshot!(&data);
}

#[test]
fn group3_1d() {
    let flags = Flags {
        ..Default::default()
    };
    let decoder = Decoder {
        algorithm: Algorithm::Group3_1D,
        flags,
        width: 81,
        rows: Some(26),
    };
    // extracted by `dump-pdf stream -f pdf.js/test/pdfs/ccitt_EndOfBlock_false.pdf 9 --raw`
    let data = decoder.decode(include_bytes!("./group3-1d")).unwrap();
    assert_eq!((81 * 26 + 7) / 8, data.len());
    insta::assert_debug_snapshot!(&data);
}

#[test]
fn group3_1d_false_end_of_block() {
    let flags = Flags {
        end_of_block: false,
        ..Default::default()
    };
    let decoder = Decoder {
        algorithm: Algorithm::Group3_1D,
        flags,
        width: 81,
        rows: Some(26),
    };
    // extracted by `dump-pdf stream -f pdf.js/test/pdfs/ccitt_EndOfBlock_false.pdf 8 --raw`
    let data = 
        decoder
            .decode(include_bytes!("group3-1d-false-end-of-block"))
            .unwrap();
    assert_eq!((81 * 26 + 7) / 8, data.len());
    insta::assert_debug_snapshot!(
        data
    );
}

#[test]
fn group3_2d() {
    let flags = Flags {
        end_of_block: false,
        ..Default::default()
    };
    let decoder = Decoder {
        algorithm: Algorithm::Group3_2D(1),
        flags,
        width: 81,
        rows: Some(26),
    };
    // extracted by `dump-pdf stream -f pdf.js/test/pdfs/ccitt_EndOfBlock_false.pdf 10 --raw`
    insta::assert_debug_snapshot!(decoder.decode(include_bytes!("group3-2d")).unwrap());
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
