mod ccitt;
pub mod file;
pub mod function;
pub mod graphics;
pub mod object;
pub mod parser;
mod run_length;
pub mod text;
use prescript::ParserError;
use snafu::Snafu;
use std::error::Error;

pub type Result<T, E = ObjectValueError> = std::result::Result<T, E>;

/// Error logging if the result is an error, panic in debug mode
pub fn log_err<E: Error>(v: Result<(), E>) {
    if let Err(e) = v {
        log::error!("{}", e);

        #[allow(clippy::panic)]
        {
            #[cfg(debug_assertions)]
            panic!("{}", snafu::Report::from_error(e));
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
    #[snafu(transparent)]
    ParseError { source: ParserError },
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
    #[snafu(whatever, display("{message}"))]
    GenericError {
        message: String,

        // Having a `source` is optional, but if it is present, it must
        // have this specific attribute and type:
        #[snafu(source(from(Box<dyn Error + Send + Sync>, Some)))]
        source: Option<Box<dyn Error + Send + Sync>>,
    },
}
