use map_macro::hash_map;
use nom::number::complete::be_i16;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;

use super::*;
use nom::combinator::opt;
use test_case::test_case;

#[test_case(&[0x8b] => 0)]
#[test_case(&[0xef] => 100)]
#[test_case(&[0x27] => -100)]
#[test_case(&[0xfa, 0x7c] => 1000)]
#[test_case(&[0xfe, 0x7c] => -1000)]
#[test_case(&[0x1c, 0x27, 0x10] => 10000)]
#[test_case(&[0x1c, 0xd8, 0xf0] => -10000)]
#[test_case(&[0x1d, 0x00, 0x01, 0x86, 0xa0] => 100000)]
#[test_case(&[0x1d, 0xff, 0xfe, 0x79, 0x60] => -100000)]
fn test_parse_integer(buf: &[u8]) -> i32 {
    let mut buf = buf.to_owned();
    buf.push(0x8b);
    let (remains, r) = parse_integer(&buf[..]).unwrap();
    assert_eq!(remains.len(), 1);
    r
}

#[test_case(&[0x1e, 0xe2, 0xa2, 0x5f] , -2.25)]
#[test_case(&[0x1e, 0x0a, 0x14, 0x05, 0x41, 0xc3, 0xff] , 0.140541e-3)]
fn test_parse_real(buf: &[u8], exp: f32) {
    let mut buf = buf.to_owned();
    buf.push(0x8b);
    let (remains, r) = parse_real(&buf[..]).unwrap();
    assert_eq!(remains.len(), 1);
    assert!((r - exp).abs() < 1e-6);
}

#[test_case(&[1] => Operator::new(1))]
#[test_case(&[21] => Operator::new(21))]
#[test_case(&[12, 0] => Operator::escaped(0))]
#[test_case(&[12, 21] => Operator::escaped(21))]
fn test_parse_operator(buf: &[u8]) -> Operator {
    let mut buf = buf.to_owned();
    buf.push(0x8b);
    let (remains, r) = parse_operator(&buf[..]).unwrap();
    assert_eq!(remains.len(), 1);
    r
}

#[test]
fn test_parse_off_size() {
    let mut buf = [0u8; 1];
    buf[0] = 1;
    let (remains, r) = parse_off_size(&buf[..]).unwrap();
    assert_eq!(remains.len(), 0);
    assert_eq!(r, OffSize::One);

    buf[0] = 2;
    let (remains, r) = parse_off_size(&buf[..]).unwrap();
    assert_eq!(remains.len(), 0);
    assert_eq!(r, OffSize::Two);

    buf[0] = 3;
    let (remains, r) = parse_off_size(&buf[..]).unwrap();
    assert_eq!(remains.len(), 0);
    assert_eq!(r, OffSize::Three);

    buf[0] = 4;
    let (remains, r) = parse_off_size(&buf[..]).unwrap();
    assert_eq!(remains.len(), 0);
    assert_eq!(r, OffSize::Four);

    buf[0] = 5;
    let e = parse_off_size(&buf[..]);
    assert!(e.is_err());
}

#[test_case(OffSize::One => 1)]
#[test_case(OffSize::Two => 2)]
#[test_case(OffSize::Three => 3)]
#[test_case(OffSize::Four => 4)]
fn off_size_len(off_size: OffSize) -> usize {
    off_size.len()
}

#[test]
fn test_parse_header() {
    let mut buf = [0u8; 4];
    buf[0] = 1;
    buf[1] = 0;
    buf[2] = 4;
    buf[3] = 1;
    let (remains, r) = parse_header(&buf[..]).unwrap();
    assert_eq!(remains.len(), 0);
    assert_eq!(
        r,
        Header {
            major: 1,
            minor: 0,
            hdr_size: 4,
            off_size: OffSize::One,
        }
    );
}

#[test_case(&[0x1c, 0x27, 0x10] => Operand::Integer(10000))]
#[test_case(&[0x1e, 0xe2, 0xa2, 0x5f] => Operand::Real(-2.25))]
#[test_case(&[0x8b, 0xef] => Operand::IntArray(vec![0, 100]))]
#[test_case(&[0x1e, 0xe2, 0xa2, 0x5f, 0x1e, 0xe2, 0xa2, 0x5f] => Operand::RealArray(vec![-2.25, -2.25]))]
fn test_parse_operand(buf: &[u8]) -> Operand {
    let mut buf = buf.to_owned();
    buf.push(12);
    let (remains, r) = parse_operand(&buf[..]).unwrap();
    assert_eq!(remains.len(), 1);
    r
}

#[test]
fn test_operator_hash() {
    fn hash(v: impl Hash) -> u64 {
        let mut hasher = DefaultHasher::new();
        v.hash(&mut hasher);
        hasher.finish()
    }

    assert_eq!(hash(1_u8), hash(Operator::new(1)));
    assert_eq!(hash(21_u8), hash(Operator::new(21)));
    assert_eq!(hash(0x80_u8 | 0), hash(Operator::escaped(0)));
}

#[test]
fn test_parse_dict() {
    // empty dict
    let (remains, r) = opt(parse_dict)(&[]).unwrap();
    assert_eq!(remains.len(), 0);
    assert_eq!(r, None);

    // dict with one item
    let buf = [0x8b_u8, 1];
    let (remains, r) = parse_dict(&buf[..]).unwrap();
    assert_eq!(remains.len(), 0);
    assert_eq!(
        r,
        Dict(hash_map! {
            Operator::new(1) => Operand::Integer(0),
        })
    );

    // dict with two items
    let buf = [0x8b_u8, 1, 0xef, 2];
    let (remains, r) = parse_dict(&buf[..]).unwrap();
    assert_eq!(remains.len(), 0);
    assert_eq!(
        r,
        Dict(hash_map! {
            Operator::new(1) => Operand::Integer(0),
            Operator::new(2) => Operand::Integer(100),
        })
    );
}

#[test]
fn dict_as_int() {
    let d = Dict(hash_map! {
        Operator::new(1) => Operand::Integer(0),
        Operator::new(2) => Operand::Real(100.0),
    });

    assert_eq!(d.as_int(Operator::new(1)), Ok(Some(0)));
    assert_eq!(d.as_int(Operator::new(3)), Ok(None));
    assert_eq!(d.as_int(Operator::new(2)), Err(Error::ExpectInt));
}

#[test]
fn dict_as_real() {
    let d = Dict(hash_map! {
        Operator::new(1) => Operand::Integer(0),
        Operator::new(2) => Operand::Real(100.0),
        Operator::new(3) => Operand::IntArray(vec![]),
    });

    assert_eq!(d.as_real(Operator::new(1)), Ok(Some(0.0)));
    assert_eq!(d.as_real(Operator::new(4)), Ok(None));
    assert_eq!(d.as_real(Operator::new(2)), Ok(Some(100.0)));
    assert_eq!(d.as_real(Operator::new(3)), Err(Error::ExpectReal));
}

#[test]
fn dict_as_int_array() {
    let d = Dict(hash_map! {
        Operator::new(1) => Operand::Integer(0),
        Operator::new(2) => Operand::IntArray(vec![1, 2, 3]),
        Operator::new(3) => Operand::RealArray(vec![]),
    });

    assert_eq!(d.as_int_array(Operator::new(1)), Err(Error::ExpectIntArray));
    assert_eq!(d.as_int_array(Operator::new(4)), Ok(None));
    assert_eq!(
        d.as_int_array(Operator::new(2)),
        Ok(Some(&[1i32, 2, 3][..]))
    );
    assert_eq!(d.as_int_array(Operator::new(3)), Err(Error::ExpectIntArray));
}

#[test]
fn dict_as_real_array() {
    let d = Dict(hash_map! {
        Operator::new(1) => Operand::Integer(0),
        Operator::new(2) => Operand::RealArray(vec![1.0, 2.0, 3.0]),
        Operator::new(3) => Operand::IntArray(vec![]),
    });

    assert_eq!(
        d.as_real_array(Operator::new(1)),
        Err(Error::ExpectRealArray)
    );
    assert_eq!(d.as_real_array(Operator::new(4)), Ok(None));
    assert_eq!(
        d.as_real_array(Operator::new(2)),
        Ok(Some(&[1.0, 2.0, 3.0][..]))
    );
    assert_eq!(
        d.as_real_array(Operator::new(3)),
        Err(Error::ExpectRealArray)
    );
}

#[test]
fn dict_as_bool() {
    let d = Dict(hash_map! {
        Operator::new(1) => Operand::Integer(0),
        Operator::new(2) => Operand::Integer(1),
        Operator::new(3) => Operand::Real(2.0),
        Operator::new(4) => Operand::Integer(2),
    });

    assert_eq!(d.as_bool(Operator::new(1)), Ok(Some(false)));
    assert_eq!(d.as_bool(Operator::new(2)), Ok(Some(true)));
    assert_eq!(d.as_bool(Operator::new(3)), Err(Error::ExpectBool));
    assert_eq!(d.as_bool(Operator::new(4)), Err(Error::ExpectBool));
    assert_eq!(d.as_bool(Operator::new(5)), Ok(None));
}

#[test]
fn dict_as_delta_encoded() {
    let d = Dict(hash_map! {
        Operator::new(1) => Operand::RealArray(vec![]),
        Operator::new(2) => Operand::RealArray(vec![1.]),
        Operator::new(3) => Operand::RealArray(vec![1., 2., 3.]),
    });

    assert_eq!(d.as_delta_encoded(Operator::new(4)), Ok(None));
    assert_eq!(d.as_delta_encoded(Operator::new(1)), Ok(Some(vec![])));
    assert_eq!(d.as_delta_encoded(Operator::new(2)), Ok(Some(vec![1.])));
    assert_eq!(
        d.as_delta_encoded(Operator::new(3)),
        Ok(Some(vec![1., 3., 6.]))
    );
}

#[test]
fn offsets() {
    // empty offsets
    let offsets = Offsets::new(OffSize::One, &[1_u8][..]).unwrap();
    assert_eq!(offsets.len(), 0);
    assert_eq!(offsets.get(0), 0);

    // two offsets
    let offsets = Offsets::new(OffSize::One, &[1_u8, 20, 30][..]).unwrap();
    assert_eq!(offsets.len(), 2);
    assert_eq!(offsets.get(0), 0);
    assert_eq!(offsets.get(2), 29);
    assert_eq!(0..19, offsets.range(0));
    assert_eq!(19..29, offsets.range(1));
}

#[test]
fn test_parse_indexed_data() {
    // empty index off_size 2 bytes
    let (remains, r) = parse_indexed_data(&[0_u8, 0, 2, 0, 1][..]).unwrap();
    assert_eq!(remains.len(), 0);
    assert_eq!(0, r.len());

    // index with one items, off_size 3 bytes
    let (remains, r) = parse_indexed_data(&[0_u8, 1, 3, 0, 0, 1, 0, 0, 2, 10][..]).unwrap();
    assert_eq!(remains.len(), 0);
    assert_eq!(1, r.len());
    assert_eq!(0..1, r.offsets.range(0));
}

#[test]
fn indexed_data_get() {
    let indexed_data = IndexedData {
        offsets: Offsets::new(OffSize::One, &[1_u8, 3][..]).unwrap(),
        data: &[0x1, 0x2],
    };
    assert_eq!(0x102i16, indexed_data.get(0, be_i16).unwrap());
}

#[test]
fn indexed_data_get_str() {
    let indexed_data = IndexedData {
        offsets: Offsets::new(OffSize::One, &[1_u8, 3, 6][..]).unwrap(),
        data: b"ab\0de",
    };
    assert_eq!(b"ab", indexed_data.get_bin_str(0));
    assert_eq!(b"\0de", indexed_data.get_bin_str(1));
}

#[test]
fn indexed_data_get_dict() {
    let indexed_data = IndexedData {
        offsets: Offsets::new(OffSize::One, &[1_u8, 3, 6][..]).unwrap(),
        data: &[0x8b_u8, 1, 0xef, 2, 0x8b, 3, 0x8b, 4],
    };
    assert_eq!(
        Dict(hash_map! {
            Operator::new(1) => Operand::Integer(0),
        }),
        indexed_data.get_dict(0)
    );
}

#[test]
fn string_index() {
    let indexed_data = IndexedData {
        offsets: Offsets::new(OffSize::One, &[1_u8, 3, 6][..]).unwrap(),
        data: b"abcde",
    };
    let index = StringIndex(indexed_data);

    assert_eq!(".notdef", index.get(0));
    assert_eq!("Semibold", index.get(390));
    assert_eq!("ab", index.get(391));
    assert_eq!("cde", index.get(392));
}

#[test]
fn name_index() {
    let indexed_data = IndexedData {
        offsets: Offsets::new(OffSize::One, &[1_u8, 3, 6, 7][..]).unwrap(),
        data: b"ab\0def",
    };
    let index = NameIndex(indexed_data);

    assert_eq!(3, index.len());
    assert_eq!(Some("ab"), index.get(0));
    assert_eq!(None, index.get(1));
    assert_eq!(Some("f"), index.get(2));
}

#[test]
fn parse_charsets_format0() {
    let (remains, r) = parse_charsets(&[0_u8, 0, 1, 0, 2][..], 3).unwrap();
    assert_eq!(remains.len(), 0);
    assert_eq!(r, Charsets::Format0(vec![1, 2]));
}

#[test]
fn parse_charsets_format1_2() {
    let (remains, r) = parse_charsets(&[1_u8, 1, 2, 0, 0, 8, 1][..], 4).unwrap();
    assert_eq!(remains.len(), 0);
    assert_eq!(r, Charsets::Format1(vec![0x102..=0x102, 8..=9]));

    let (remains, r) = parse_charsets(&[2_u8, 1, 2, 0, 0, 0, 8, 0, 1][..], 4).unwrap();
    assert_eq!(remains.len(), 0);
    assert_eq!(r, Charsets::Format2(vec![0x102..=0x102, 8..=9]));
}

#[test]
fn parse_encodings_format0() {
    // without supplement
    let (remains, r) = parse_encodings(&[0_u8, 3, 1, 0, 2][..]).unwrap();
    assert_eq!(remains.len(), 0);
    assert_eq!(r, (Encodings::Format0(vec![1, 0, 2]), None));

    // with supplement
    let (remains, r) = parse_encodings(&[0x80_u8, 3, 1, 0, 2, 1, 1, 2, 1][..]).unwrap();
    assert_eq!(remains.len(), 0);
    assert_eq!(
        r,
        (
            Encodings::Format0(vec![1, 0, 2]),
            Some(vec![EncodingSupplement::new(1, 0x201)]),
        )
    );
}

#[test]
fn parse_encodings_format1() {
    // without supplement
    let (remains, r) = parse_encodings(&[1_u8, 2, 1, 10, 2, 20][..]).unwrap();
    assert_eq!(remains.len(), 0);
    assert_eq!(
        r,
        (
            Encodings::Format1(vec![EncodingRange::new(1, 10), EncodingRange::new(2, 20)]),
            None
        )
    );

    // with supplement
    let (remains, r) = parse_encodings(&[0x81_u8, 1, 1, 100, 1, 1, 2, 1][..]).unwrap();
    assert_eq!(remains.len(), 0);
    assert_eq!(
        r,
        (
            Encodings::Format1(vec![EncodingRange::new(1, 100)]),
            Some(vec![EncodingSupplement::new(1, 0x201)])
        )
    );
}

#[test]
fn encoding_supplement_apply() {
    let mut encodings: [Option<&str>; 256] = [None; 256];
    encodings[100] = Some("foo");
    encodings[101] = Some("bar");
    let string_index = StringIndex(IndexedData {
        offsets: Offsets::new(OffSize::One, &[1_u8, 3, 6][..]).unwrap(),
        data: b"abcde",
    });
    let supp = EncodingSupplement::new(100, 10);
    supp.apply(string_index, &mut encodings);
    assert_eq!(encodings[100], Some(STANDARD_STRINGS[10]));
    assert_eq!(encodings[101], Some("bar"));
}
