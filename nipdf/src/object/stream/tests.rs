use super::*;
use crate::{
    file::{decode_stream, test_file},
    function::Domain,
    object::Name,
};
use miniz_oxide::deflate::compress_to_vec;
use std::{rc::Rc, str::from_utf8};
use test_case::test_case;

#[test_case([] => Ok(vec![]); "empty")]
#[test_case(
    [(KEY_FILTER, 1.into())] => matches Err(ObjectValueError::UnexpectedType);
    "incorrect filter type"
)]
#[test_case(
    [(KEY_FILTER, Object::Array(vec![1.into()].into()))] => matches Err(_);
    "filter is array but item not name"
)]
#[test_case(
    [(KEY_FILTER, FILTER_FLATE_DECODE.into())] =>
    Ok(vec![(FILTER_FLATE_DECODE, None)]);
     "one filter"
)]
#[test_case(
    [(KEY_FILTER, FILTER_FLATE_DECODE.into()),
     (KEY_FILTER_PARAMS, Object::Array(vec![Object::Null].into()))] =>
    Ok(vec![(FILTER_FLATE_DECODE, None)]);
     "one filter with null params in array"
)]
#[test_case(
    [(KEY_FILTER, FILTER_FLATE_DECODE.into()),
     (KEY_FILTER_PARAMS, Object::Dictionary(Dictionary::default()))] =>
    Ok(vec![(FILTER_FLATE_DECODE, Some(Dictionary::default()))]);
     "one filter with dictionary params"
)]
#[test_case(
    [(KEY_FILTER, vec![
        FILTER_FLATE_DECODE.into(),
        FILTER_DCT_DECODE.into(),
    ].into())] =>
    Ok(vec![(FILTER_FLATE_DECODE, None),
            (FILTER_DCT_DECODE, None)]);
     "two filters no params"
)]
#[test_case(
    [(KEY_FILTER, Object::Array(vec![
        FILTER_FLATE_DECODE.into(),
        FILTER_DCT_DECODE.into(),
    ].into())),
    (KEY_FILTER_PARAMS, Dictionary::default().into())] =>
    Ok(vec![(FILTER_FLATE_DECODE, Some(Dictionary::default())),
            (FILTER_DCT_DECODE, None)]);
     "two filters with null params"
)]
#[test_case(
    [(KEY_FFILTER, FILTER_FLATE_DECODE.into())] =>
    Err(ObjectValueError::ExternalStreamNotSupported);
     "filter not supported"
)]
fn test_iter_filter(
    dict: impl IntoIterator<Item = (Name, Object)>,
) -> Result<Vec<(Name, Option<Dictionary>)>, ObjectValueError> {
    let dict: Dictionary = dict.into_iter().collect::<Dictionary>();
    let d = FilterDict::new(&dict, None)?;
    let r: Vec<(Name, Option<Dictionary>)> =
        iter_filters(d)?.map(|(k, v)| (k, v.cloned())).collect();
    Ok(r)
}

#[test_case(228, 228, 227 => 228)]
#[test_case(0, 0, 1 => 0; "close to a")]
#[test_case(0, 0, 2 => 0; "close to b")]
#[test_case(0, 0, 3 => 0; "close to c")]
fn test_paeth(a: u8, b: u8, c: u8) -> u8 {
    paeth(a, b, c)
}

#[test]
fn predictor() {
    let exp_image =
        image::io::Reader::open(test_file("sample_files/filters/predictor-exp.png")).unwrap();
    let exp_image = exp_image.decode().unwrap();
    let width = exp_image.width();
    let exp = exp_image.into_bytes();

    let decoded = decode_stream("sample_files/filters/predictor.pdf", 23, |d, resolver| {
        let params = d.get(&sname("DecodeParms")).unwrap().as_dict()?;
        assert_eq!(
            15,
            resolver
                .resolve_container_value(params, &sname("Predictor"))?
                .int()?
        );
        assert_eq!(3, params["Colors"].int().unwrap());
        assert_eq!(8, params["BitsPerComponent"].int().unwrap());
        Ok(())
    })
    .unwrap();
    assert_eq!(exp.len(), decoded.len());
    for (line, (exp, act)) in exp
        .chunks(width as usize * 3)
        .zip(decoded.chunks(width as usize * 3))
        .enumerate()
    {
        let exp = hex::encode(exp);
        let act = hex::encode(act);
        assert_eq!(exp, act, "line {line} differs");
    }
}

#[test]
fn image_mask_try_from_object() {
    // ColorKeyMask
    #[rustfmt::skip]
    let o = Object::Array(
        vec![
            0.into(), 1.into(), // domain 1
            0.1.into(), 0.9.into(), // domain 2
            0.2.into(), 0.8.into(), // domain 3
        ].into(),
    );
    let mask = ImageMask::try_from(&o).unwrap();
    assert_eq!(
        mask,
        ImageMask::ColorKey(Domains(vec![
            Domain::new(0., 1.),
            Domain::new(0.1, 0.9),
            Domain::new(0.2, 0.8),
        ]))
    );

    // ExplicitMask
    let stream = Rc::new(Stream(
        Dictionary::default(),
        // b"0 1 2 3 4 5 6 7 8 9".as_ref(),
        BufPos::new(0, None),
        ObjectId::empty(),
    ));
    let o = Object::Stream(stream.clone());
    let mask = ImageMask::try_from(&o).unwrap();
    assert_eq!(mask, ImageMask::Explicit(stream));
}

#[test_case([10, 15, 20, 0] => true; "matches lower range")]
#[test_case([110, 115, 120, 0] => true; "matches upper range")]
#[test_case([10, 15, 20, 1] => true; "alpha not compared")]
#[test_case([109, 114, 120, 0] => true; "in range")]
#[test_case([9, 15, 20, 0] => false; "red part less than min")]
#[test_case([111, 115, 120, 0] => false; "red part greater than max")]
#[test_case([10, 14, 20, 0] => false; "green part less than min")]
#[test_case([110, 116, 120, 0] => false; "green part greater than max")]
fn test_color_matches_color_key(color: [u8; 4]) -> bool {
    let color_key: ColorKey = ([10, 15, 20, 0], [110, 115, 120, 0]);
    color_matches_color_key(color_key, color)
}

#[test]
fn test_color_key_range() {
    let range = Domains(vec![
        Domain::new(10., 110.),
        Domain::new(15., 115.),
        Domain::new(20., 120.),
    ]);
    let color_key: ColorKey = ([10, 15, 20, 255], [110, 115, 120, 255]);
    assert_eq!(color_key, color_key_range(&range, &ColorSpace::DeviceRGB));
}

#[test_case(b"" => b"".as_slice())]
#[test_case(b"AB" => b"\xab".as_slice(); "upper case")]
#[test_case(b"ab" => b"\xab".as_slice(); "lower case")]
#[test_case(b"A B" => b"\xab".as_slice(); "ignore whitespace")]
#[test_case(b"AB12>" => b"\xab\x12".as_slice(); "EOD")]
#[test_case(b"AB1>" => b"\xab\x10".as_slice(); "EOD with odd hex digits")]
fn test_decode_ascii_hex(buf: &[u8]) -> Vec<u8> {
    decode_ascii_hex(buf).unwrap()
}

#[test]
fn test_deflate() {
    // with zlib header and invalid adler32
    let input = include_bytes!("zlib-no-adler32");
    let data = deflate(input).unwrap();
    // assert that data is valid ascii char bytes
    assert!(data.iter().all(|&b| b.is_ascii()));

    // no zlib header(only deflate data)
    let input = compress_to_vec(&data, 1);
    let back = deflate(&input).unwrap();
    assert_eq!(data, back);
}

#[test]
fn deflate_recover_truncated_zlib_data() {
    let input = include_bytes!("deflate-stream-recover");
    let exp = include_bytes!("deflate-stream-recover.exp");
    let exp = from_utf8(exp).unwrap();
    let data = deflate(input).unwrap();
    let s = from_utf8(&data).unwrap();
    assert_eq!(s, exp);
}
