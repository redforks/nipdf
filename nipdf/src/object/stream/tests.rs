use crate::file::decode_stream;

use super::*;
use itertools::Itertools;
use test_case::test_case;

#[test_case([] => Ok(vec![]); "empty")]
#[test_case(
    [(KEY_FILTER, 1.into())] => matches Err(ObjectValueError::UnexpectedType);
    "incorrect filter type"
)]
#[test_case(
    [(KEY_FILTER, Object::Array(vec![1.into()]))] => matches Err(_);
    "filter is array but item not name"
)]
#[test_case(
    [(KEY_FILTER, Name::borrowed(B_FILTER_FLATE_DECODE).into())] =>
    Ok(vec![(FILTER_FLATE_DECODE.to_owned(), None)]);
     "one filter"
)]
#[test_case(
    [(KEY_FILTER, Name::borrowed(B_FILTER_FLATE_DECODE).into()),
     (KEY_FILTER_PARAMS, Object::Null)] =>
    Ok(vec![(FILTER_FLATE_DECODE.to_owned(), None)]);
     "one filter with null params"
)]
#[test_case(
    [(KEY_FILTER, Name::borrowed(B_FILTER_FLATE_DECODE).into()),
     (KEY_FILTER_PARAMS, Object::Array(vec![Object::Null]))] =>
    Ok(vec![(FILTER_FLATE_DECODE.to_owned(), None)]);
     "one filter with null params in array"
)]
#[test_case(
    [(KEY_FILTER, Name::borrowed(B_FILTER_FLATE_DECODE).into()),
     (KEY_FILTER_PARAMS, Object::Dictionary(Dictionary::default()))] =>
    Ok(vec![(FILTER_FLATE_DECODE.to_owned(), Some(Dictionary::default()))]);
     "one filter with dictionary params"
)]
#[test_case(
    [(KEY_FILTER, vec![
        Name::borrowed(B_FILTER_FLATE_DECODE).into(),
        Name::borrowed(B_FILTER_DCT_DECODE).into(),
    ].into())] =>
    Ok(vec![(FILTER_FLATE_DECODE.to_owned(), None),
            (FILTER_DCT_DECODE.to_owned(), None)]);
     "two filters no params"
)]
#[test_case(
    [(KEY_FILTER, vec![
        Name::borrowed(B_FILTER_FLATE_DECODE).into(),
        Name::borrowed(B_FILTER_DCT_DECODE).into(),
    ].into()),
    (KEY_FILTER_PARAMS, Dictionary::default().into())] =>
    Ok(vec![(FILTER_FLATE_DECODE.to_owned(), Some(Dictionary::default())),
            (FILTER_DCT_DECODE.to_owned(), None)]);
     "two filters with null params"
)]
#[test_case(
    [(KEY_FFILTER, Name::borrowed(B_FILTER_FLATE_DECODE).into())] =>
    Err(ObjectValueError::ExternalStreamNotSupported);
     "filter not supported"
)]
fn test_iter_filter(
    dict: impl IntoIterator<Item = (&'static [u8], Object<'static>)>,
) -> Result<Vec<(String, Option<Dictionary<'static>>)>, ObjectValueError> {
    let dict: Dictionary<'static> = dict
        .into_iter()
        .map(|(k, v)| (Name::borrowed(k), v))
        .collect::<Dictionary>();
    let stream = Stream(dict, &[]);
    let r: Vec<(String, Option<Dictionary<'static>>)> = stream
        .iter_filter()?
        .map(|(k, v)| (k.to_owned(), v.cloned()))
        .collect_vec();
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
fn predictor_8bit() {
    insta::assert_debug_snapshot!(
        &decode_stream("filters/predictor.pdf", 7u32, |d, resolver| {
            let params = d.get(&b"DecodeParms"[..]).unwrap().as_dict()?;
            assert_eq!(
                15,
                resolver
                    .resolve_container_value(params, "Predictor")?
                    .as_int()?
            );
            Ok(())
        })
        .unwrap()[127..255]
    );
}
