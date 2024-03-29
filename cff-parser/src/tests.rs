use super::*;
use prescript::NOTDEF;

fn sample_cff() -> &'static [u8] {
    include_bytes!("sample.cff")
}

#[test]
fn open_file() {
    let file = File::open(sample_cff()).unwrap();
    assert_eq!(1, file.major_version());
    assert_eq!(0, file.minor_version());
}

#[test]
fn iter_fonts() {
    let file = File::open(sample_cff()).unwrap();
    let fonts: Vec<_> = file.iter().unwrap().collect();
    assert_eq!(1, fonts.len());
    assert_eq!("PAPHHO+MyriadPro-Regular", fonts[0].name());
}

#[test]
fn font_encodings() {
    let file = File::open(sample_cff()).unwrap();
    let fonts: Vec<_> = file.iter().unwrap().collect();
    assert_eq!(1, fonts.len());
    let encodings = fonts[0].encodings().unwrap();
    assert_eq!(NOTDEF, encodings.get_str(0));
    assert_eq!("space", encodings.get_str(32));
}

#[test]
fn font_encodings68() {
    let file = File::open(include_bytes!("68.cff")).unwrap();
    let fonts: Vec<_> = file.iter().unwrap().collect();
    assert_eq!(1, fonts.len());
    let encodings = fonts[0].encodings().unwrap();
    assert_eq!("minus", encodings.get_str(0));
}
