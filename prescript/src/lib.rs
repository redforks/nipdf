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
    error::{AddContext, ErrorConvert, ErrorKind, FromExternalError, ParseError},
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
    #[snafu(display("{}/{:?}", kind, context))]
    Leaf {
        kind: ErrorKind,
        context: Vec<&'static str>,
    },
    #[snafu(display("{}/{:?}", kind, context))]
    Inter {
        kind: ErrorKind,
        context: Vec<&'static str>,
        #[snafu(source(from(ParserError, Box::new)))]
        source: Box<ParserError>,
    },
    Other {
        kind: ErrorKind,
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

impl<I, E: Error + Send + Sync + 'static> FromExternalError<I, E> for ParserError {
    fn from_external_error(_: &I, kind: ErrorKind, e: E) -> Self {
        Self::Other {
            kind,
            context: vec![],
            source: Box::new(e),
        }
    }
}
