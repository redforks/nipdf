use super::*;
use crate::file::open_test_file;
use insta::assert_debug_snapshot;
use test_case::test_case;
use test_log::test;

#[test]
fn parse_file_header() {
    let buf = b"%PDF-1.7\n%comment\n";
    let (buf, header) = parse_header(buf).unwrap();
    assert_eq!(buf, b"");
    assert_eq!(header, "1.7");
}

#[test_case(None, b"hello", b"world"; "not exist")]
#[test_case(Some(0), b"hello", b"hello"; "matches")]
#[test_case(Some(1), b"\nhello", b"hello"; "after newline")]
#[test_case(Some(1), b"\nhello\n", b"hello"; "end with newline")]
#[test_case(Some(2), b"\r\nhello\r\n", b"hello"; "CRLF")]
#[test_case(Some(4), b"foo\nfoo\nbar", b"foo"; "from end")]
#[test_case(None, b"abc-foo", b"foo"; "not the whole line")]
fn test_r_find_start_object_tag(exp: Option<usize>, buf: &[u8], tag: &[u8]) {
    assert_eq!(exp, r_find_start_object_tag(buf, tag));
}

#[test]
fn test_parse_trailer() {
    let buf = b"trailer\n<< /Size 1 >>\nstartxref\n1234\n%%EOF";
    assert_debug_snapshot!(parse_trailer(buf).unwrap());
}

#[test_case(b"xref\n1 0\n", "empty")]
#[test_case(
    b"xref\n1 2\n0000000000 00000 n \n0000000010 00000 n \n",
    "two entries"
)]
fn test_parse_xref_table(buf: impl AsRef<[u8]>, name: &str) {
    assert_debug_snapshot!(name, parse_xref_table(buf.as_ref()).unwrap());
}

#[test]
fn test_parse_frame() {
    assert_debug_snapshot!(
        parse_frame(
            b"xref
1 2
0000000000 00000 n
0000000010 00000 n
trailer
<< /Size 1 >>
startxref
1234
%%EOF
"
        )
        .unwrap()
    );
}

#[test]
fn test_parse_frame_set() {
    assert_debug_snapshot!(
        parse_frame_set(
            b"%PDF-1.7
xref
1 1
0000000000 00000 n
trailer
<< /Size 1 >>
startxref
9
%%EOF
xref
1 1
0000000000 00000 n
trailer
<< /Prev 9 >>
startxref
77
%%EOF
"
        )
        .unwrap()
    );
}

#[test]
fn read_xref_stream() {
    let f = open_test_file("sample_files/file-structure/xref-stream.pdf");
    let resolver = f.resolver().unwrap();

    // assert object in file
    assert_eq!(
        sname("Catalog"),
        resolver
            .resolve(1)
            .unwrap()
            .as_dict()
            .unwrap()
            .get(&sname("Type"))
            .unwrap()
            .name()
            .unwrap()
    );

    // assert object in stream
    assert_eq!(
        1,
        resolver
            .resolve(2)
            .unwrap()
            .as_dict()
            .unwrap()
            .get(&sname("Count"))
            .unwrap()
            .int()
            .unwrap()
    );
}

#[test]
fn read_xref_stream_has_prev() {
    let f = open_test_file("pdf.js/test/pdfs/160F-2019.pdf");
    // get resolver without error
    f.resolver().unwrap();
}
