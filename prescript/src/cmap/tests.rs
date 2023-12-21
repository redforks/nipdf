use super::*;
use tinyvec::array_vec;
use CharCode::*;

fn one(v: u8) -> CharCode {
    One(v)
}

fn two(v: u16) -> CharCode {
    Two((v >> 8) as u8, v as u8)
}

fn three(v: u32) -> CharCode {
    Three((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

fn four(v: u32) -> CharCode {
    Four((v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8, v as u8)
}

#[test]
fn char_code_as_byte_slice() {
    assert_eq!(&[0x20][..], one(0x20).as_ref());
    assert_eq!(&[0x81, 0x40][..], two(0x8140).as_ref());
    assert_eq!(&[0x00, 0x81, 0x40][..], three(0x8140).as_ref());
    assert_eq!(&[0xD8, 0x00, 0xDC, 0x00][..], four(0xD800DC00).as_ref());
}

#[test]
fn code_range_parse() {
    assert_eq!(
        CodeRange::parse("20", "7e").unwrap(),
        CodeRange(array_vec!([ByteRange; 4] => ByteRange::new(0x20, 0x7e)))
    );
    assert_eq!(
        CodeRange::parse("8140", "817e").unwrap(),
        CodeRange(array_vec!([ByteRange; 4] =>
            ByteRange::new(0x81, 0x81),
            ByteRange::new(0x40, 0x7e)
        ))
    );
    assert_eq!(
        CodeRange::parse("D800DC00", "DBFFDFFF").unwrap(),
        CodeRange(array_vec!([ByteRange; 4] =>
            ByteRange::new(0xD8, 0xDB),
            ByteRange::new(0x00, 0xFF),
            ByteRange::new(0xDC, 0xDF),
            ByteRange::new(0x00, 0xFF),
        )),
    );
}

#[test]
fn code_range_in_range() {
    let r = CodeRange::parse("20", "7e").unwrap();
    assert!(r.in_range(one(0x20)));
    assert!(r.in_range(one(0x7e)));
    assert!(r.in_range(one(0x21)));
    assert!(!r.in_range(one(0x1f)));
    assert!(!r.in_range(one(0x7f)));
    assert!(!r.in_range(two(0x7f)));

    let r = CodeRange::parse("8140", "817e").unwrap();
    assert!(r.in_range(two(0x8140)));
    assert!(r.in_range(two(0x817e)));
    assert!(r.in_range(two(0x8141)));
    assert!(!r.in_range(two(0x8040)));
    assert!(!r.in_range(one(0x81)));
    assert!(!r.in_range(three(0x8140)));
}

#[test]
fn code_range_next_code() {
    // one
    let r = CodeRange::parse("20", "7e").unwrap();
    assert_eq!(CodeSpaceResult::Matched(one(0x20)), r.next_code(&[0x20, 0]));
    assert_eq!(CodeSpaceResult::Matched(one(0x7e)), r.next_code(&[0x7e]));
    assert_eq!(CodeSpaceResult::Matched(one(0x21)), r.next_code(&[0x21]));
    assert_eq!(CodeSpaceResult::NotMatched, r.next_code(&[0x1f]));
    assert_eq!(CodeSpaceResult::NotMatched, r.next_code(&[0x7f]));

    // two
    let r = CodeRange::parse("8140", "817e").unwrap();
    assert_eq!(
        CodeSpaceResult::Matched(two(0x8140)),
        r.next_code(&[0x81, 0x40])
    );
    assert_eq!(
        CodeSpaceResult::Matched(two(0x817e)),
        r.next_code(&[0x81, 0x7e])
    );
    assert_eq!(
        CodeSpaceResult::Matched(two(0x8141)),
        r.next_code(&[0x81, 0x41])
    );
    assert_eq!(CodeSpaceResult::NotMatched, r.next_code(&[0x80, 0x40]));
    assert_eq!(
        CodeSpaceResult::Partial(two(0x817f)),
        r.next_code(&[0x81, 0x7f])
    );
    assert_eq!(CodeSpaceResult::Partial(one(0x81)), r.next_code(&[0x81]));
}

#[test]
fn code_space() {
    let code_space = CodeSpace::new(vec![
        CodeRange::parse("20", "7e").unwrap(),
        CodeRange::parse("8140", "817e").unwrap(),
        CodeRange::parse("D800DC00", "DBFFDFFF").unwrap(),
        CodeRange::parse("E000", "FFFF").unwrap(),
    ]);

    // matches
    assert_eq!(
        (&[0u8][..], Ok(one(0x20))),
        code_space.next_code(&[0x20, 0])
    );

    // not match
    assert_eq!(
        (&[0u8][..], Err(one(0x1f))),
        code_space.next_code(&[0x1f, 0])
    );

    // four bytes matched
    assert_eq!(
        (&[][..], Ok(four(0xD800DC00))),
        code_space.next_code(&[0xD8, 0x00, 0xDC, 0x00])
    );

    // two bytes partial matched
    assert_eq!(
        (&[][..], Err(two(0x817f))),
        code_space.next_code(&[0x81, 0x7f]),
    );
}

#[test]
fn single_code_map() {
    let m = SingleCodeMap::new(one(0x20), CID(0x1234));
    assert_eq!(Some(CID(0x1234)), m.map(one(0x20)));
    assert_eq!(None, m.map(one(0x21)));
}

#[test]
fn range_map_to_one() {
    let m = RangeMapToOne {
        range: CodeRange::parse("20", "7e").unwrap(),
        cid: CID(0x1234),
    };
    assert_eq!(Some(CID(0x1234)), m.map(one(0x20)));
    assert_eq!(Some(CID(0x1234)), m.map(one(0x7e)));
    assert_eq!(None, m.map(one(0x12)));
    assert_eq!(None, m.map(two(0x21)));
}

#[test]
fn inc_range_map() {
    // one byte
    let m = IncRangeMap {
        range: CodeRange::parse("20", "7e").unwrap(),
        start_cid: CID(1234),
    };
    assert_eq!(Some(CID(1234)), m.map(one(0x20)));
    assert_eq!(Some(CID(1235)), m.map(one(0x21)));
    assert_eq!(Some(CID(1236)), m.map(one(0x22)));
    assert_eq!(None, m.map(one(0x1f)));
    assert_eq!(None, m.map(one(0x7f)));

    // two bytes
    let m = IncRangeMap {
        range: CodeRange::parse("8100", "827f").unwrap(),
        start_cid: CID(1000),
    };
    assert_eq!(Some(CID(1000)), m.map(two(0x8100)));
    assert_eq!(Some(CID(1001)), m.map(two(0x8101)));
    assert_eq!(Some(CID(1128)), m.map(two(0x8200)));
    assert_eq!(Some(CID(1255)), m.map(two(0x827f)));
}
