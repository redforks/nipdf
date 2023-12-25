use super::*;
use crate::{name, sname};
use either::{Left, Right};
use std::vec;
use test_log::test;
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
        CodeRange::parse("8140", "817e").unwrap(),
        CodeRange::parse("D800DC00", "DBFFDFFF").unwrap(),
        CodeRange::parse("E000", "FFFF").unwrap(),
    ]);

    // matches
    assert_eq!(
        (&[][..], Right(two(0x8141))),
        code_space.next_code(&[0x81, 0x41])
    );

    // not match
    assert_eq!(
        (&[][..], Left(two(0x1f01))),
        code_space.next_code(&[0x1f, 1])
    );

    // not enough bytes
    assert_eq!((&[][..], Left(two(0x1f00))), code_space.next_code(&[0x1f]));

    // four bytes matched
    assert_eq!(
        (&[][..], Right(four(0xD800DC00))),
        code_space.next_code(&[0xD8, 0x00, 0xDC, 0x00])
    );

    // two bytes partial matched
    assert_eq!(
        (&[][..], Left(two(0x817f))),
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

#[test]
fn mapper() {
    let mapper = Mapper {
        ranges: vec![
            IncRangeMap {
                range: CodeRange::parse("20", "7e").unwrap(),
                start_cid: CID(1234),
            },
            IncRangeMap {
                range: CodeRange::parse("8100", "827f").unwrap(),
                start_cid: CID(1000),
            },
        ]
        .into(),
        chars: vec![
            SingleCodeMap::new(one(0x7f), CID(0x0000)),
            SingleCodeMap::new(one(0x20), CID(0x1234)),
        ]
        .into(),
    };

    // no matches
    assert_eq!(None, mapper.map(one(0x1f)));

    // matches single cid
    assert_eq!(Some(CID(0x0000)), mapper.map(one(0x7f)));

    // single cid has high priority than range if both matches
    assert_eq!(Some(CID(0x1234)), mapper.map(one(0x20)));

    // matches range 1
    assert_eq!(Some(CID(1235)), mapper.map(one(0x21)));
    // matches range 2
    assert_eq!(Some(CID(1002)), mapper.map(two(0x8102)));
}

#[test]
fn cmap() {
    let code_space = CodeSpace::new(vec![
        CodeRange::parse("20", "7e").unwrap(),
        CodeRange::parse("8140", "817e").unwrap(),
        CodeRange::parse("D800DC00", "DBFFDFFF").unwrap(),
        CodeRange::parse("E000", "FFFF").unwrap(),
    ]);
    let cid_map = Mapper {
        ranges: vec![].into(),
        chars: vec![SingleCodeMap::new(two(0x8144), CID(0x1234))].into(),
    };
    let notdef_map = Mapper {
        ranges: vec![].into(),
        chars: vec![
            SingleCodeMap::new(one(0x7f), CID(1)),
            SingleCodeMap::new(one(0x0), CID(2)),
        ]
        .into(),
    };

    let cmap = CMap {
        cid_system_info: Default::default(),
        w_mode: Default::default(),
        name: sname("foo"),
        code_space,
        cid_map,
        notdef_map,
        use_map: None,
    };

    assert_eq!(
        vec![
            // not in code space range, and not in notdef range, returns 0
            CID(0),
            // not in code space range, but in notdef range,
            CID(2),
            // cid mapped
            CID(0x1234),
            // in code space range, notdef mapped
            CID(1),
            // in code space range, no cid mapping, and notdef not mapped
            CID(0),
        ],
        cmap.map(&[1u8, 0, 0x81, 0x44, 0x7f, 0x81, 0x50])
    );
}

#[test]
fn use_map() {
    let base_code_space = CodeSpace::new(vec![CodeRange::parse("20", "30").unwrap()]);
    let code_space = CodeSpace::new(vec![CodeRange::parse("25", "40").unwrap()]);
    let base_cid_map = Mapper {
        ranges: vec![].into(),
        chars: vec![SingleCodeMap::new(one(0x30), CID(0x500))].into(),
    };
    let cid_map = Mapper {
        ranges: vec![].into(),
        chars: vec![SingleCodeMap::new(one(0x35), CID(0x501))].into(),
    };
    let base_notdef_map = Mapper {
        ranges: vec![].into(),
        chars: vec![SingleCodeMap::new(one(0x20), CID(1))].into(),
    };
    let notdef_map = Mapper {
        ranges: vec![].into(),
        chars: vec![
            SingleCodeMap::new(one(0x30), CID(2)),
            SingleCodeMap::new(one(0x36), CID(20)),
        ]
        .into(),
    };

    let use_map = CMap {
        cid_system_info: Default::default(),
        w_mode: Default::default(),
        name: sname("base"),
        code_space: base_code_space,
        cid_map: base_cid_map,
        notdef_map: base_notdef_map,
        use_map: None,
    };

    let cmap = CMap {
        cid_system_info: Default::default(),
        w_mode: Default::default(),
        name: sname("foo"),
        code_space,
        cid_map,
        notdef_map,
        use_map: Some(Rc::new(use_map)),
    };

    assert_eq!(
        vec![
            // in current cid map
            CID(0x501),
            // cid map failed, use base cid map
            CID(0x500),
            // both cid map failed, use current notdef map
            CID(20),
            // both cid map failed, use base notdef map if current notdef map failed
            CID(1),
            // use default undef, if both cid and notdef failed
            CID(0),
        ],
        cmap.map(&[0x35u8, 0x30, 0x36, 0x20, 0x7f])
    );
}

#[test]
fn parse_cmap_file() {
    let mut reg = CMapRegistry::new();
    let cmap_data = include_bytes!("test-cmap.ps");
    let cmap = reg.add_cmap_file(cmap_data).unwrap();
    assert_eq!("Test-H", cmap.name.as_str());
    assert_eq!(
        CIDSystemInfo {
            registry: "Testing".to_owned(),
            ordering: "Test".to_owned(),
            supplement: 3,
        },
        cmap.cid_system_info
    );
    assert_eq!(WriteMode::Horizontal, cmap.w_mode);
    assert_eq!(
        &CodeSpace::new(vec![
            CodeRange::parse("00", "80").unwrap(),
            CodeRange::parse("8740", "fefe").unwrap(),
        ]),
        &cmap.code_space,
    );

    assert_eq!(544, cmap.cid_map.chars.len());
    assert_eq!(
        SingleCodeMap::new(two(0x8943), CID(17718)),
        cmap.cid_map.chars[0]
    );
    assert_eq!(
        SingleCodeMap::new(two(0xfedd), CID(7080)),
        cmap.cid_map.chars[543]
    );

    assert_eq!(665, cmap.cid_map.ranges.len());
    assert_eq!(
        IncRangeMap {
            range: CodeRange::parse("20", "7e").unwrap(),
            start_cid: CID(1),
        },
        cmap.cid_map.ranges[0]
    );
    assert_eq!(
        IncRangeMap {
            range: CodeRange::parse("feef", "fefe").unwrap(),
            start_cid: CID(17144),
        },
        cmap.cid_map.ranges[664]
    );

    assert_eq!(2, cmap.notdef_map.ranges.len());
    assert_eq!(
        RangeMapToOne {
            range: CodeRange::parse("00", "0f").unwrap(),
            cid: CID(1),
        },
        cmap.notdef_map.ranges[0]
    );
    assert_eq!(
        RangeMapToOne {
            range: CodeRange::parse("10", "1f").unwrap(),
            cid: CID(2),
        },
        cmap.notdef_map.ranges[1]
    );

    assert_eq!(2, cmap.notdef_map.chars.len());
    assert_eq!(
        SingleCodeMap::new(two(0x8940), CID(7717)),
        cmap.notdef_map.chars[0]
    );
    assert_eq!(
        SingleCodeMap::new(one(0x45), CID(17717)),
        cmap.notdef_map.chars[1]
    );
}

#[test]
fn parse_cmap_file_with_use() {
    let mut reg = CMapRegistry::new();
    let base_data = include_bytes!("test-cmap.ps");
    let use_cmap_data = include_bytes!("use-cmap.ps");
    let base = reg.add_cmap_file(base_data).unwrap();
    let use_cmap = reg.add_cmap_file(use_cmap_data).unwrap();
    assert_eq!(base, use_cmap.use_map.as_ref().unwrap().clone());
}

#[test]
fn get_builtin_cmap() {
    let reg = CMapRegistry::new();

    // get cmap that use another builtin cmap
    let cmap = reg.get(&sname("ETen-B5-V")).unwrap();
    assert_eq!("ETen-B5-V", cmap.name.as_str());
    assert_eq!(
        "ETen-B5-H",
        cmap.as_ref().use_map.as_ref().unwrap().name.as_str()
    );

    // assert all builtin cmaps
    for &n in PREDEFINED_CMAPS.keys() {
        let cmap = reg.get(&name(n)).unwrap();
        assert_eq!(n, cmap.name.as_str());
    }
}

#[test]
fn identity_h_map() {
    let reg = CMapRegistry::new();
    let cmap = reg.get(&sname("Identity-H")).unwrap();
    assert_eq!(vec![CID(0x04)], cmap.map(&[0x0, 0x04u8]));
}
