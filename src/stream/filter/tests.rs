use crate::object::new_name;

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
    assert_eq!(vec![0, 1, 2], result);
}

#[test]
fn decode_one_filter() {
    let dict = Dictionary::from_iter([(KEY_FILTER, new_name(FILTER_ZERO_DECODER))].into_iter());
    let stream = Stream::new(dict, vec![0, 1, 2]);
    assert_eq!(vec![0, 0, 0], decode(&stream).unwrap());
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
                Dictionary::from_iter(
                    [(FILTER_INC_DECODER_STEP_PARAM, Object::Integer(2))].into_iter(),
                )
                .into(),
            ),
        ]
        .into_iter(),
    );
    let stream = Stream::new(dict, vec![0, 1, 2]);
    assert_eq!(vec![2, 2, 2], decode(&stream).unwrap());
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

#[test]
fn decode_external_stream() {
    let dict = Dictionary::from_iter([(KEY_FFILTER, new_name(FILTER_ZERO_DECODER))].into_iter());
    let stream = Stream::new(dict, vec![0, 1, 2]);
    let err = decode(&stream).unwrap_err();
    assert!(matches!(err, DecodeError::ExternalStreamNotSupported))
}
