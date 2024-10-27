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
