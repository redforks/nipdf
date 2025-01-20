mod ccitt;
pub mod file;
pub mod function;
pub mod graphics;
pub mod object;
pub mod parser;
mod run_length;
pub mod text;
use prescript::ParserError;
pub use prescript::{AnyWhatever, Result};
use snafu::{Report, Snafu};
use std::error::Error;

/// Error logging if the result is an error, panic in debug mode
pub fn log_err<E: Error>(v: Result<(), E>) {
    if let Err(e) = v {
        log::error!("{}", e);

        #[allow(clippy::panic)]
        {
            #[cfg(debug_assertions)]
            panic!("{}", Report::from_error(e));
        }
    }
}

#[derive(Debug, Snafu)]
pub enum ObjectValueError {
    #[snafu(display("unexpected type"))]
    UnexpectedType,
    #[snafu(display("invalid hex string"))]
    InvalidHexString,
    #[snafu(display("invalid name format"))]
    InvalidNameFormat,
    #[snafu(display("Name not in dictionary"))]
    DictNameMissing,
    #[snafu(display("Reference target not found"))]
    ReferenceTargetNotFound,
    #[snafu(display("External stream not supported"))]
    ExternalStreamNotSupported,
    #[snafu(display("Unknown filter"))]
    UnknownFilter,
    #[snafu(display("Filter decode error"))]
    FilterDecodeError,
    #[snafu(display("Stream not image"))]
    StreamNotImage,
    #[snafu(display("Stream is not bytes"))]
    StreamIsNotBytes,
    #[snafu(display("Stream length not defined"))]
    StreamLengthNotDefined,
    #[snafu(display("Object not found by id {id}"))]
    ObjectIDNotFound { id: object::RuntimeObjectId },
    #[snafu(display("Parse error: {message}"))]
    ParseError { message: String },
    #[snafu(display("Unexpected dict schema type, schema: {schema}"))]
    DictSchemaUnExpectedType { schema: String },
    #[snafu(display("Dict schema error, schema: {schema}, key: {key}"))]
    DictSchemaError {
        schema: String,
        key: prescript::Name,
    },
    #[snafu(display("Graphics operation schema error"))]
    GraphicsOperationSchemaError,
    #[snafu(display("Dict key not found"))]
    DictKeyNotFound,
    /// Has error when resolve object, but not include `ObjectValueError` in source.
    #[snafu(display("Object resolve error: {source}"))]
    ObjectResolveError {
        #[snafu(source(from(ObjectValueError, Box::new)))]
        source: Box<ObjectValueError>,
    },
    #[snafu(whatever, display("{message}"))]
    GenericError {
        message: String,

        // Having a `source` is optional, but if it is present, it must
        // have this specific attribute and type:
        #[snafu(source(from(Box<dyn Error + Send + Sync>, Some)))]
        source: Option<Box<dyn Error + Send + Sync>>,
    },
}

impl ObjectValueError {
    /// Return true if error is `ObjectValueError::ObjectResolveError`, or any source error is
    /// `ObjectValueError::ObjectResolveError`.
    pub fn is_object_resolve_error(err: &(dyn Error + 'static)) -> bool {
        if let Some(obj_err) = err.downcast_ref::<ObjectValueError>() {
            match obj_err {
                // Direct match for ObjectResolveError
                ObjectValueError::ObjectResolveError { .. } => true,

                // Recursively check source for GenericError
                ObjectValueError::GenericError { source, .. } => {
                    if let Some(source) = source {
                        Self::is_object_resolve_error(source.as_ref())
                    } else {
                        false
                    }
                }

                // All other error variants
                _ => false,
            }
        } else {
            err.source()
                .map_or(false, |source| Self::is_object_resolve_error(source))
        }
    }
}

impl<I: winnow::stream::AsBStr, E: std::fmt::Display> From<winnow::error::ParseError<I, E>>
    for ObjectValueError
{
    fn from(e: winnow::error::ParseError<I, E>) -> Self {
        Self::ParseError {
            message: format!("{}", e),
        }
    }
}

impl<E: std::fmt::Debug> From<winnow::error::ErrMode<E>> for ObjectValueError {
    fn from(e: winnow::error::ErrMode<E>) -> Self {
        Self::ParseError {
            message: format!("{}", e),
        }
    }
}

#[cfg(test)]
mod tests;
