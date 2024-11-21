#![warn(clippy::unwrap_used)]
#![warn(clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]
#![cfg_attr(test, allow(clippy::expect_used))]

use snafu::{Snafu, Whatever};
use winnow::{
    error::{AddContext, ErrorConvert, ErrorKind, ParseError, StrContext},
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

pub type PResult<O> = winnow::PResult<O, ParserError>;

#[derive(Snafu, Debug)]
pub enum ParserError<C: 'static = StrContext> {
    Leaf {
        kind: ErrorKind,
        context: Vec<C>,
    },
    Inter {
        kind: ErrorKind,
        context: Vec<C>,
        #[snafu(source(from(ParserError<C>, Box::new)))]
        source: Box<ParserError<C>>,
    },
    Other {
        kind: ErrorKind,
        context: Vec<C>,
        source: Box<dyn std::error::Error>,
    },
}

impl<C: 'static, I> From<ParseError<I, ParserError<C>>> for ParserError<C> {
    fn from(value: ParseError<I, ParserError<C>>) -> Self {
        value.into_inner()
    }
}

impl<C: 'static> ErrorConvert<ParserError<C>> for ParserError<C> {
    fn convert(self) -> ParserError<C> {
        self
    }
}

impl<I: Stream, C> AddContext<I, C> for ParserError<C> {
    fn add_context(mut self, _input: &I, _token_start: &<I as Stream>::Checkpoint, c: C) -> Self {
        match self {
            Self::Leaf {
                ref mut context, ..
            }
            | Self::Inter {
                ref mut context, ..
            }
            | Self::Other {
                ref mut context, ..
            } => {
                context.push(c);
            }
        }
        self
    }
}

impl<I: Stream> winnow::error::ParserError<I> for ParserError {
    fn from_error_kind(_: &I, kind: ErrorKind) -> Self {
        Self::Leaf {
            kind,
            context: Vec::new(),
        }
    }

    fn append(self, _: &I, _: &<I as Stream>::Checkpoint, kind: ErrorKind) -> Self {
        Self::Inter {
            kind,
            context: vec![],
            source: Box::new(self),
        }
    }
}
