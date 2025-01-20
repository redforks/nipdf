use super::*;
use snafu::FromString as _;

#[test]
fn direct_object_resolve_error() {
    let error = ObjectValueError::ObjectResolveError {
        source: Box::new(ObjectValueError::UnexpectedType),
    };
    assert!(ObjectValueError::is_object_resolve_error(&error));
}

#[test]
fn nested_object_resolve_error() {
    let nested_error = ObjectValueError::ObjectResolveError {
        source: Box::new(ObjectValueError::UnexpectedType),
    };
    let error = ObjectValueError::GenericError {
        message: "outer error".to_string(),
        source: Some(Box::new(nested_error)),
    };
    let error = AnyWhatever::with_source(Box::new(error), "outer outer".to_owned());
    assert!(ObjectValueError::is_object_resolve_error(&error));
}

#[test]
fn non_object_resolve_error() {
    let error = ObjectValueError::UnexpectedType;
    assert!(!ObjectValueError::is_object_resolve_error(&error));
}

#[test]
fn generic_error_without_source() {
    let error = ObjectValueError::GenericError {
        message: "test error".to_string(),
        source: None,
    };
    assert!(!ObjectValueError::is_object_resolve_error(&error));
}

#[test]
fn generic_error_with_non_object_value_error_source() {
    let error = ObjectValueError::GenericError {
        message: "test error".to_string(),
        source: Some(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            "io error",
        ))),
    };
    assert!(!ObjectValueError::is_object_resolve_error(&error));
}

#[test]
fn deeply_nested_object_resolve_error() {
    let innermost = ObjectValueError::ObjectResolveError {
        source: Box::new(ObjectValueError::UnexpectedType),
    };
    let inner = ObjectValueError::GenericError {
        message: "inner".to_string(),
        source: Some(Box::new(innermost)),
    };
    let outer = ObjectValueError::GenericError {
        message: "outer".to_string(),
        source: Some(Box::new(inner)),
    };
    assert!(ObjectValueError::is_object_resolve_error(&outer));
}

#[test]
fn deeply_nested_non_object_resolve_error() {
    let innermost = ObjectValueError::UnexpectedType;
    let inner = ObjectValueError::GenericError {
        message: "inner".to_string(),
        source: Some(Box::new(innermost)),
    };
    let outer = ObjectValueError::GenericError {
        message: "outer".to_string(),
        source: Some(Box::new(inner)),
    };
    assert!(!ObjectValueError::is_object_resolve_error(&outer));
}
