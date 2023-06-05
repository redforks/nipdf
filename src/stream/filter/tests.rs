use crate::object::new_name;
use itertools::Itertools;

use super::super::{
    FILTER_INC_DECODER, FILTER_INC_DECODER_STEP_PARAM, FILTER_ZERO_DECODER, KEY_DECODE_PARMS,
    KEY_FFILTER, KEY_FILTER,
};
use super::*;
use lopdf::{Dictionary, Object, Stream};

#[test]
fn decode_no_filter() {
    let stream = Stream::new(Dictionary::new(), vec![0, 1, 2]);
    let result = decode(&stream).unwrap();
    assert_eq!([0, 1, 2].as_slice(), &result as &[u8]);
}

#[test]
fn decode_one_filter() {
    let dict = Dictionary::from_iter([(KEY_FILTER, new_name(FILTER_ZERO_DECODER))].into_iter());
    let stream = Stream::new(dict, vec![0, 1, 2]);
    assert_eq!([0, 0, 0].as_slice(), &decode(&stream).unwrap() as &[u8]);
}

#[test]
fn decode_two_fiters() {
    let dict = Dictionary::from_iter(
        [
            (
                KEY_FILTER,
                vec![new_name(FILTER_ZERO_DECODER), new_name(FILTER_INC_DECODER)].into(),
            ),
            (
                KEY_DECODE_PARMS,
                vec![
                    Object::Null,
                    Dictionary::from_iter(
                        [(FILTER_INC_DECODER_STEP_PARAM, Object::Integer(2))].into_iter(),
                    )
                    .into(),
                ]
                .into(),
            ),
        ]
        .into_iter(),
    );
    let stream = Stream::new(dict, vec![0, 1, 2]);
    assert_eq!([2, 2, 2].as_slice(), &decode(&stream).unwrap() as &[u8]);
}

#[test]
fn decode_unknown_filter() {
    let dict = Dictionary::from_iter([(KEY_FILTER, new_name("unknown"))].into_iter());
    let stream = Stream::new(dict, vec![0, 1, 2]);
    let err = decode(&stream).unwrap_err();
    match err {
        DecodeError::UnknownFilter(filter) => assert_eq!(filter, "unknown"),
        _ => panic!("Unexpected error: {:?}", err),
    }
}

macro_rules! assert_decode_error_result {
    ($rv: expr, $exp: pat_param) => {
        match $rv {
            Err($exp) => (),
            Err(err) => panic!("Unexpected error: {:?}", err),
            Ok(_) => panic!("Should not succeed"),
        }
    };
}

#[test]
fn decode_external_stream() {
    let dict = Dictionary::from_iter([(KEY_FFILTER, new_name(FILTER_ZERO_DECODER))].into_iter());
    let stream = Stream::new(dict, vec![0, 1, 2]);
    assert_decode_error_result!(decode(&stream), DecodeError::ExternalStreamNotSupported);
}

#[test]
fn test_iter_filter() {
    // invalid Filter entry type
    let dict = Dictionary::from_iter([(KEY_FILTER, Object::Integer(0))]);
    assert_decode_error_result!(iter_filter(&dict), DecodeError::InvalidFilterObjectType);

    // filter value is array, but items not name
    let dict = Dictionary::from_iter([(KEY_FILTER, Object::Array(vec![Object::Integer(0)]))]);
    assert_decode_error_result!(iter_filter(&dict), DecodeError::InvalidFilterObjectType);

    // params are not array or name
    let dict = Dictionary::from_iter(
        [
            (KEY_FILTER, new_name("zero")),
            (KEY_DECODE_PARMS, Object::Integer(0)),
        ]
        .into_iter(),
    );
    assert_decode_error_result!(iter_filter(&dict), DecodeError::InvalidParamsObjectType);

    // params are array, but items not name
    let dict = Dictionary::from_iter(
        [
            (KEY_FILTER, new_name("zero")),
            (KEY_DECODE_PARMS, Object::Array(vec![Object::Integer(0)])),
        ]
        .into_iter(),
    );
    assert_decode_error_result!(iter_filter(&dict), DecodeError::InvalidParamsObjectType);

    // empty
    let dict = Dictionary::new();
    assert_eq!(iter_filter(&dict).unwrap().count(), 0);

    // one filter no params
    let dict = Dictionary::from_iter([(KEY_FILTER, new_name("zero"))].into_iter());
    assert_eq!(
        iter_filter(&dict).unwrap().collect_vec(),
        vec![(b"zero".as_slice(), None)]
    );

    // one filter with Null params
    let dict = Dictionary::from_iter(
        [
            (KEY_FILTER, new_name("zero")),
            (KEY_DECODE_PARMS, Object::Null),
        ]
        .into_iter(),
    );
    assert_eq!(
        iter_filter(&dict).unwrap().collect_vec(),
        vec![(b"zero".as_slice(), None)]
    );

    // one filter with params
    let dict = Dictionary::from_iter(
        [
            (KEY_FILTER, new_name("zero")),
            (KEY_DECODE_PARMS, Dictionary::new().into()),
        ]
        .into_iter(),
    );
    assert_eq!(
        iter_filter(&dict).unwrap().collect_vec(),
        vec![(b"zero".as_slice(), Some(&Dictionary::new()))]
    );

    // two filters no params
    let dict =
        Dictionary::from_iter([(KEY_FILTER, vec![new_name("zero"), new_name("inc")].into())]);
    assert_eq!(
        iter_filter(&dict).unwrap().collect_vec(),
        vec![(b"zero".as_slice(), None), (b"inc".as_slice(), None)]
    );

    // two filters with params
    let dict = Dictionary::from_iter(
        [
            (KEY_FILTER, vec![new_name("zero"), new_name("inc")].into()),
            (KEY_DECODE_PARMS, Dictionary::new().into()),
        ]
        .into_iter(),
    );
    let empty_dict = Dictionary::new();
    assert_eq!(
        iter_filter(&dict).unwrap().collect_vec(),
        vec![
            (b"zero".as_slice(), Some(&empty_dict)),
            (b"inc".as_slice(), None)
        ]
    );

    // two filters first params is Null
    let dict = Dictionary::from_iter(
        [
            (KEY_FILTER, vec![new_name("zero"), new_name("inc")].into()),
            (
                KEY_DECODE_PARMS,
                vec![Object::Null, Dictionary::new().into()].into(),
            ),
        ]
        .into_iter(),
    );
    assert_eq!(
        iter_filter(&dict).unwrap().collect_vec(),
        vec![
            (b"zero".as_slice(), None),
            (b"inc".as_slice(), Some(&Dictionary::new()))
        ]
    );
}
