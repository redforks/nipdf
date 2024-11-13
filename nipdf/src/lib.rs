#![warn(clippy::unwrap_used)]
#![warn(clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]
#![cfg_attr(test, allow(clippy::expect_used))]

use snafu::Whatever;

mod ascii85;
mod ccitt;
pub mod file;
pub mod function;
pub mod graphics;
pub mod object;
pub mod parser;
mod run_length;
pub mod text;

type Result<T, E = Whatever> = std::result::Result<T, E>;

/// Error logging if the result is an error, panic in debug mode
pub fn log_err<E: std::error::Error>(v: Result<(), E>) {
    if let Err(e) = v {
        log::error!("{}", e);

        #[cfg(debug_assertions)]
        panic!("{}", e);
    }
}

trait ResultExt<T, E>: Sized {
    /// Convert the result to a unit result
    fn remove_result(self) -> Result<(), E>;
}

impl<T, E> ResultExt<T, E> for Result<T, E> {
    fn remove_result(self) -> Result<(), E> {
        self.map(|_| ())
    }
}
