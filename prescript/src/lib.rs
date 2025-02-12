use snafu::{
    AsBacktrace, AsErrorSource, Backtrace, ErrorCompat, FromString, GenerateImplicitData, Snafu,
};
use std::error::Error;

pub(crate) mod machine;
pub(crate) mod parser;

pub mod ascii85;
pub mod cmap;
mod encoding;
mod pdf_fn;
mod type1;
pub use encoding::Encoding;
pub use pdf_fn::PdfFunc;
pub use type1::Font;
use winnow::{
    error::{AddContext, ErrorConvert, FromExternalError, ParseError},
    stream::Stream,
};

/// PostScript Name Value
pub type Name = kstring::KStringBase<Box<str>>;

/// Create Name from `&str`
#[inline]
#[must_use]
pub fn name(s: &str) -> Name {
    Name::from_ref(s)
}

#[inline]
#[must_use]
pub const fn sname(s: &'static str) -> Name {
    Name::from_static(s)
}

/// Symbol for .notdef glyph
pub const NOTDEF: &str = ".notdef";

/// Like [snafu::Whatever], but implement [Send + Sync]
#[derive(Debug, Snafu)]
#[snafu(crate_root(crate))]
#[snafu(whatever)]
#[snafu(display("{message}"))]
#[snafu(provide(opt, ref, chain, dyn std::error::Error => source.as_deref()))]
pub struct AnyWhatever {
    #[snafu(source(from(Box<dyn Error + Send + Sync>, Some)))]
    #[snafu(provide(false))]
    source: Option<Box<dyn Error + Send + Sync>>,
    message: String,
    backtrace: Backtrace,
}

pub type Result<T, E = AnyWhatever> = std::result::Result<T, E>;

#[derive(Snafu, Debug)]
pub enum ParserError {
    #[snafu(display("{:?}", context))]
    Leaf { context: Vec<&'static str> },
    #[snafu(display("{:?}", context))]
    Inter {
        context: Vec<&'static str>,
        #[snafu(source(from(ParserError, Box::new)))]
        source: Box<ParserError>,
    },
    #[snafu(display("{:?}", context))]
    Other {
        context: Vec<&'static str>,
        source: Box<dyn Error + Sync + Send + 'static>,
    },
}

impl<I> From<ParseError<I, ParserError>> for ParserError {
    fn from(value: ParseError<I, ParserError>) -> Self {
        value.into_inner()
    }
}

impl ErrorConvert<ParserError> for ParserError {
    fn convert(self) -> ParserError {
        self
    }
}

impl<I: Stream> AddContext<I, &'static str> for ParserError {
    fn add_context(
        mut self,
        _input: &I,
        _token_start: &<I as Stream>::Checkpoint,
        c: &'static str,
    ) -> Self {
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
    type Inner = Self;

    fn append(self, _: &I, _: &<I as Stream>::Checkpoint) -> Self {
        Self::Inter {
            context: vec![],
            source: Box::new(self),
        }
    }

    fn from_input(_input: &I) -> Self {
        Self::Leaf { context: vec![] }
    }

    fn into_inner(self) -> winnow::Result<Self::Inner, Self> {
        Ok(self)
    }
}

impl<I, E: Error + Send + Sync + 'static> FromExternalError<I, E> for ParserError {
    fn from_external_error(_: &I, e: E) -> Self {
        Self::Other {
            context: vec![],
            source: Box::new(e),
        }
    }
}
