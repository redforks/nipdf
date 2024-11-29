#![warn(clippy::unwrap_used)]
#![warn(clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]
#![cfg_attr(test, allow(clippy::expect_used))]

use snafu::{Error, Snafu, Whatever};
use winnow::{
    error::{AddContext, ErrorConvert, ErrorKind, FromExternalError, ParseError},
    stream::Stream,
};

mod ascii85;
mod ccitt;
pub mod file;
pub mod function;
pub mod graphics;
pub mod object;
pub mod parser;
mod run_length;
pub mod text;
use prescript::{AnyWhatever, ParserError, Result};

/// Error logging if the result is an error, panic in debug mode
pub fn log_err<E: Error>(v: Result<(), E>) {
    if let Err(e) = v {
        log::error!("{}", e);

        #[cfg(debug_assertions)]
        panic!("{}", e);
    }
}
