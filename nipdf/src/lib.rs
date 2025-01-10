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
use std::error::Error;

/// Error logging if the result is an error, panic in debug mode
pub fn log_err<E: Error>(v: Result<(), E>) {
    if let Err(e) = v {
        log::error!("{}", e);

        #[allow(clippy::panic)]
        {
            #[cfg(debug_assertions)]
            panic!("{}", e);
        }
    }
}
