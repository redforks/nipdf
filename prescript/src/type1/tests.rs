use super::*;
use test_log::test;

#[test]
fn parse_pfb_file() {
    let data = include_bytes!("../../../nipdf/fonts/d050000l.pfb");
    let font = Font::parse(data).unwrap();
    assert_eq!("Dingbats", font.header.font_name);
}

#[test]
fn parse_file_header_loose_ending() {
    let data = include_bytes!("file-header-loose-ending.pfb");
    let font = Font::parse(data).unwrap();
    assert_eq!("NewsGothicStd-Bold", font.header.font_name);
    assert!(font.encoding().is_some());
}

#[test]
fn parse_pfa_file() {
    let data = include_bytes!("p052024l.pfa");
    let font = Font::parse(data).unwrap();
    assert_eq!("URWPalladioL-BoldItal", font.header.font_name);
}

#[test]
fn parse_std_14_fonts_file() {
    let files: [&[u8]; 14] = [
        include_bytes!("../../../nipdf/fonts/d050000l.pfb"),
        include_bytes!("../../../nipdf/fonts/n019003l.pfb"),
        include_bytes!("../../../nipdf/fonts/n019004l.pfb"),
        include_bytes!("../../../nipdf/fonts/n019023l.pfb"),
        include_bytes!("../../../nipdf/fonts/n019024l.pfb"),
        include_bytes!("../../../nipdf/fonts/n021003l.pfb"),
        include_bytes!("../../../nipdf/fonts/n021004l.pfb"),
        include_bytes!("../../../nipdf/fonts/n021023l.pfb"),
        include_bytes!("../../../nipdf/fonts/n021024l.pfb"),
        include_bytes!("../../../nipdf/fonts/n022003l.pfb"),
        include_bytes!("../../../nipdf/fonts/n022004l.pfb"),
        include_bytes!("../../../nipdf/fonts/n022023l.pfb"),
        include_bytes!("../../../nipdf/fonts/n022024l.pfb"),
        include_bytes!("../../../nipdf/fonts/s050000l.pfb"),
    ];
    let file_names: [&str; 14] = [
        "Dingbats",
        "NimbusSanL-Regu",
        "NimbusSanL-Bold",
        "NimbusSanL-ReguItal",
        "NimbusSanL-BoldItal",
        "NimbusRomNo9L-Regu",
        "NimbusRomNo9L-Medi",
        "NimbusRomNo9L-ReguItal",
        "NimbusRomNo9L-MediItal",
        "NimbusMonL-Regu",
        "NimbusMonL-Bold",
        "NimbusMonL-ReguObli",
        "NimbusMonL-BoldObli",
        "StandardSymL",
    ];
    for (f, name) in files.into_iter().zip(file_names) {
        let font = Font::parse(f).unwrap();
        assert_eq!(name, font.header.font_name);
    }
}
