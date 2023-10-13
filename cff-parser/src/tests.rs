use super::*;

#[test]
fn open_file() {
    let f = include_bytes!("sample.cff");
    let file = File::open(f.to_vec()).unwrap();
    assert_eq!(1, file.major_version());
    assert_eq!(0, file.minor_version());
}
