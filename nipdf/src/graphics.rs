use crate::{
    ObjectValueError,
    graphics::trans::TextToUserSpace,
    object::{
        Array, Dictionary, InlineImage, InlineStream, Object, RuntimeObjectId, Stream, TextString,
        TextStringOrNumber,
    },
    parser::{self, eol3, whitespace, wsc_prefixed0, wsc0},
};
use euclid::{Length, Point2D, Transform2D};
use log::{debug, error, warn};
use nipdf_macro::{OperationParser, TryFromIntObject, TryFromNameObject, pdf_object};
use prescript::{AnyWhatever, Name, sname};
use snafu::whatever;
use std::{
    num::ParseIntError,
    str::{Utf8Error, from_utf8},
};
use winnow::{
    PResult, Parser,
    combinator::{alt, repeat_till},
    error::{AddContext, FromExternalError, ParserError},
    seq,
    token::{any, one_of, take_till},
};

pub mod color_space;
pub mod pattern;
pub mod trans;
use self::trans::{TextPoint, TextSpace, UserToUserSpace};
pub(crate) use pattern::*;

pub mod shading;
pub use shading::{Extend, RadialCircle};

impl<S, T> TryFrom<&Object> for Transform2D<f32, S, T> {
    type Error = ObjectValueError;

    fn try_from(obj: &Object) -> Result<Self, Self::Error> {
        let arr = obj.as_arr()?;
        if arr.len() != 6 {
            return Err(ObjectValueError::UnexpectedType);
        }
        Ok(Self::new(
            arr[0].number()?,
            arr[1].number()?,
            arr[2].number()?,
            arr[3].number()?,
            arr[4].number()?,
            arr[5].number()?,
        ))
    }
}

pub type Point = euclid::default::Point2D<f32>;

#[derive(Debug, Clone, Copy, PartialEq, Default, TryFromIntObject)]
pub enum LineCapStyle {
    #[default]
    Butt = 0,
    Round = 1,
    Square = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, TryFromIntObject)]
pub enum LineJoinStyle {
    #[default]
    Miter = 0,
    Round = 1,
    Bevel = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, strum::Display, Default, TryFromNameObject)]
pub enum RenderingIntent {
    AbsoluteColorimetric,
    #[default]
    RelativeColorimetric,
    Saturation,
    Perceptual,
}

#[derive(Debug, Clone, Copy, PartialEq, TryFromIntObject)]
pub enum TextRenderingMode {
    Fill = 0,
    Stroke = 1,
    FillAndStroke = 2,
    Invisible = 3,
    FillAndClip = 4,
    StrokeAndClip = 5,
    FillStrokeAndClip = 6,
    Clip = 7,
}

/// ColorSpace use it to create RGB color.
/// It depends on the color space, for DeviceGray, the args is one number,
/// for DeviceRGB, the args is three number.
#[derive(Clone, PartialEq, Debug)]
pub struct ColorArgs(Vec<f32>);

impl AsRef<[f32]> for ColorArgs {
    fn as_ref(&self) -> &[f32] {
        self.0.as_ref()
    }
}

impl<'b> ConvertFromObject<'b> for ColorArgs {
    fn convert_from_object(objects: &'b mut Vec<Object>) -> Result<Self, ObjectValueError> {
        let mut result = Vec::with_capacity(objects.len());
        while let Some(o) = objects.pop() {
            if let Ok(num) = o.number() {
                result.push(num);
            } else {
                whatever!("color args: {:?}", o);
            }
        }
        result.reverse();
        Ok(Self(result))
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum ColorSpaceArgs {
    Name(Name),
    Array(Array),
    Ref(RuntimeObjectId),
}

impl<'b> TryFrom<&'b Object> for ColorSpaceArgs {
    type Error = ObjectValueError;

    fn try_from(object: &'b Object) -> Result<Self, Self::Error> {
        match object {
            Object::Name(name) => Ok(Self::Name(name.clone())),
            Object::Array(arr) => Ok(Self::Array(arr.clone())),
            Object::Reference(id) => Ok(Self::Ref(id.into())),
            _ => {
                error!("Can not parse ColorSpaceArgs from {:?}", object);
                Err(ObjectValueError::GraphicsOperationSchemaError)
            }
        }
    }
}

impl<'b> ConvertFromObject<'b> for ColorSpaceArgs {
    fn convert_from_object(objects: &'b mut Vec<Object>) -> Result<Self, ObjectValueError> {
        let o = objects
            .pop()
            .ok_or(ObjectValueError::GraphicsOperationSchemaError)?;
        ColorSpaceArgs::try_from(&o).map_err(|_| ObjectValueError::GraphicsOperationSchemaError)
    }
}

#[pdf_object(())]
#[root_pdf_object]
trait ICCStreamDictTrait {
    fn n(&self) -> u32;
    #[try_from]
    fn alternate(&self) -> Option<ColorSpaceArgs>;
}

impl<'b, const N: usize> ConvertFromObject<'b> for [f32; N] {
    fn convert_from_object(objects: &'b mut Vec<Object>) -> Result<Self, ObjectValueError> {
        let mut result = [0.0; N];
        for i in 0..N {
            result[N - 1 - i] = objects
                .pop()
                .ok_or(ObjectValueError::GraphicsOperationSchemaError)?
                .number()?;
        }
        Ok(result)
    }
}

impl TryFrom<&Object> for ColorArgs {
    type Error = ObjectValueError;

    fn try_from(obj: &Object) -> Result<Self, Self::Error> {
        Ok(Self(match obj {
            Object::Array(arr) => match arr.len() {
                1 => vec![arr[0].number()?],
                3 => vec![arr[0].number()?, arr[1].number()?, arr[2].number()?],
                4 => vec![
                    arr[0].number()?,
                    arr[1].number()?,
                    arr[2].number()?,
                    arr[3].number()?,
                ],
                _ => return Err(ObjectValueError::GraphicsOperationSchemaError),
            },
            _ => return Err(ObjectValueError::GraphicsOperationSchemaError),
        }))
    }
}

impl<'b, S, T> ConvertFromObject<'b> for Transform2D<f32, S, T> {
    fn convert_from_object(objects: &'b mut Vec<Object>) -> Result<Self, ObjectValueError> {
        let f = objects
            .pop()
            .ok_or(ObjectValueError::GraphicsOperationSchemaError)?
            .number()?;
        let e = objects
            .pop()
            .ok_or(ObjectValueError::GraphicsOperationSchemaError)?
            .number()?;
        let d = objects
            .pop()
            .ok_or(ObjectValueError::GraphicsOperationSchemaError)?
            .number()?;
        let c = objects
            .pop()
            .ok_or(ObjectValueError::GraphicsOperationSchemaError)?
            .number()?;
        let b = objects
            .pop()
            .ok_or(ObjectValueError::GraphicsOperationSchemaError)?
            .number()?;
        let a = objects
            .pop()
            .ok_or(ObjectValueError::GraphicsOperationSchemaError)?
            .number()?;
        Ok(Self::new(a, b, c, d, e, f))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ColorArgsOrName {
    Color(ColorArgs),
    Name((Name, Option<ColorArgs>)),
}

#[derive(Debug, Clone, PartialEq)]
pub struct NameOfDict(pub Name);

#[derive(Debug, Clone, PartialEq)]
pub enum NameOrDict {
    Name(Name),
    Dict(Dictionary),
}

#[derive(Debug, Clone, PartialEq)]
pub enum NameOrDictByRef<'b> {
    Name(&'b Name),
    Dict(&'b Dictionary),
}

impl<'b> TryFrom<&'b Object> for NameOrDictByRef<'b> {
    type Error = ObjectValueError;

    fn try_from(obj: &'b Object) -> Result<Self, Self::Error> {
        match obj {
            Object::Name(name) => Ok(NameOrDictByRef::Name(name)),
            Object::Dictionary(dict) => Ok(NameOrDictByRef::Dict(dict)),
            _ => Err(ObjectValueError::GraphicsOperationSchemaError),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum NameOrStream<'b> {
    Name(&'b Name),
    Stream(&'b Stream),
}

impl NameOrStream<'_> {
    const IDENTITY: &'static Name = &sname("Identity");

    pub const fn identity() -> Self {
        NameOrStream::Name(Self::IDENTITY)
    }
}

impl<'b> TryFrom<&'b Object> for NameOrStream<'b> {
    type Error = ObjectValueError;

    fn try_from(obj: &'b Object) -> Result<Self, Self::Error> {
        match obj {
            Object::Name(name) => Ok(NameOrStream::Name(name)),
            Object::Stream(stream) => Ok(NameOrStream::Stream(stream)),
            _ => Err(ObjectValueError::GraphicsOperationSchemaError),
        }
    }
}

#[derive(Debug, Clone, PartialEq, OperationParser)]
#[rustfmt::skip]
pub enum Operation {
    // General Graphics State Operations
    #[op_tag("w")]
    SetLineWidth(f32),
    #[op_tag("J")]
    SetLineCap(LineCapStyle),
    #[op_tag("j")]
    SetLineJoin(LineJoinStyle),
    #[op_tag("M")]
    SetMiterLimit(f32),
    #[op_tag("d")]
    SetDashPattern(Vec<f32>, f32),
    #[op_tag("ri")]
    SetRenderIntent(RenderingIntent),
    #[op_tag("i")]
    SetFlatness(f32),
    #[op_tag("gs")]
    SetGraphicsStateParameters(NameOfDict),

    // Special Graphics State Operations
    #[op_tag("q")]
    SaveGraphicsState,
    #[op_tag("Q")]
    RestoreGraphicsState,
    #[op_tag("cm")]
    ModifyCTM(UserToUserSpace),

    // Path Construction Operations
    #[op_tag("m")]
    MoveToNext(Point),
    #[op_tag("l")]
    LineToNext(Point),
    #[op_tag("c")]
    AppendBezierCurve(Point, Point, Point),
    #[op_tag("v")]
    AppendBezierCurve2(Point, Point),
    #[op_tag("y")]
    AppendBezierCurve1(Point, Point),
    #[op_tag("h")]
    ClosePath,
    #[op_tag("re")]
    AppendRectangle(Point, f32, f32),

    // Path Painting Operations
    #[op_tag("S")]
    Stroke,
    #[op_tag("s")]
    CloseAndStroke,
    #[op_tag("f")]
    FillNonZero,
    #[op_tag("F")]
    FillNonZeroDeprecated,
    #[op_tag("f*")]
    FillEvenOdd,
    #[op_tag("B")]
    FillAndStrokeNonZero,
    #[op_tag("B*")]
    FillAndStrokeEvenOdd,
    #[op_tag("b")]
    CloseFillAndStrokeNonZero,
    #[op_tag("b*")]
    CloseFillAndStrokeEvenOdd,
    #[op_tag("n")]
    EndPath,

    // Clipping Path Operations
    #[op_tag("W")]
    ClipNonZero,
    #[op_tag("W*")]
    ClipEvenOdd,

    // Text Object Operations
    #[op_tag("BT")]
    BeginText,
    #[op_tag("ET")]
    EndText,

    // Text State Operations
    #[op_tag("Tc")]
    SetCharacterSpacing(Length<f32, TextSpace>),
    #[op_tag("Tw")]
    SetWordSpacing(Length<f32, TextSpace>),
    #[op_tag("Tz")]
    SetHorizontalScaling(f32),
    #[op_tag("TL")]
    SetLeading(f32),
    #[op_tag("Tf")]
    SetFont(NameOfDict, f32),
    #[op_tag("Tr")]
    SetTextRenderingMode(TextRenderingMode),
    #[op_tag("Ts")]
    SetTextRise(f32),

    // Text Positioning Operations
    #[op_tag("Td")]
    MoveTextPosition(TextPoint),
    #[op_tag("TD")]
    MoveTextPositionAndSetLeading(TextPoint),
    #[op_tag("Tm")]
    SetTextMatrix(TextToUserSpace),
    #[op_tag("T*")]
    MoveToStartOfNextLine,

    // Text Showing Operations
    #[op_tag("Tj")]
    ShowText(TextString),
    #[op_tag("TJ")]
    ShowTexts(Vec<TextStringOrNumber>),
    #[op_tag("'")]
    MoveToNextLineAndShowText(TextString),
    #[op_tag("\"")]
    SetSpacingMoveToNextLineAndShowText(f32, f32, String),

    // Type 3 Font Operations
    #[op_tag("d0")]
    SetGlyphWidth(Point),
    #[op_tag("d1")]
    SetGlyphWidthAndBoundingBox(Point, Point, Point),

    // Color Operations
    #[op_tag("CS")]
    SetStrokeColorSpace(ColorSpaceArgs),
    #[op_tag("cs")]
    SetFillColorSpace(ColorSpaceArgs),
    #[op_tag("SC")]
    SetStrokeColor(ColorArgs),
    #[op_tag("SCN")]
    SetStrokeColorOrWithPattern(ColorArgsOrName),
    #[op_tag("sc")]
    SetFillColor(ColorArgs),
    #[op_tag("scn")]
    SetFillColorOrWithPattern(ColorArgsOrName),
    #[op_tag("G")]
    SetStrokeGray([f32; 1]), // Should be Color::Gray
    #[op_tag("g")]
    SetFillGray([f32; 1]),   // Should be Color::Gray
    #[op_tag("RG")]
    SetStrokeRGB([f32; 3]), // Should be Color::Rgb
    #[op_tag("rg")]
    SetFillRGB([f32; 3]),   // Should be Color::Rgb
    #[op_tag("K")]
    SetStrokeCMYK([f32; 4]), // Should be Color::Cmyk
    #[op_tag("k")]
    SetFillCMYK([f32; 4]),   // Should be Color::Cmyk

    // Shading Operation
    #[op_tag("sh")]
    PaintShading(NameOfDict),

    // Inline Image Operations
    #[op_tag("BI")]
    BeginInlineImage,
    #[op_tag("ID")]
    BeginInlineImageData,
    #[op_tag("EI")]
    EndInlineImage,
    #[op_tag("paint-inline-image")]
    PaintInlineImage(InlineImage),

    // XObject Operation
    #[op_tag("Do")]
    PaintXObject(NameOfDict),

    // Marked Content Operations
    #[op_tag("MP")]
    DesignateMarkedContentPoint(NameOfDict),
    #[op_tag("DP")]
    DesignateMarkedContentPointWithProperties(NameOfDict, NameOrDict),
    #[op_tag("BMC")]
    BeginMarkedContent(NameOfDict),
    #[op_tag("BDC")]
    BeginMarkedContentWithProperties(NameOfDict, NameOrDict),
    #[op_tag("EMC")]
    EndMarkedContent,

    // Compatibility Operations
    #[op_tag("BX")]
    BeginCompatibilitySection,
    #[op_tag("EX")]
    EndCompatibilitySection,
}

pub(crate) trait ConvertFromObject<'b>
where
    Self: Sized,
{
    fn convert_from_object(objects: &'b mut Vec<Object>) -> Result<Self, ObjectValueError>;
}

impl<'b, T: for<'c> ConvertFromObject<'c>> ConvertFromObject<'b> for Vec<T> {
    fn convert_from_object(objects: &'b mut Vec<Object>) -> Result<Self, ObjectValueError> {
        let mut arr: Vec<_> = objects
            .pop()
            .ok_or(ObjectValueError::GraphicsOperationSchemaError)?
            .as_arr()?
            .iter()
            .cloned()
            .collect();
        let mut result = Self::new();
        while !arr.is_empty() {
            result.push(T::convert_from_object(&mut arr)?);
        }
        result.reverse();
        Ok(result)
    }
}

impl<'b> ConvertFromObject<'b> for TextString {
    fn convert_from_object(objects: &'b mut Vec<Object>) -> Result<Self, ObjectValueError> {
        let o = objects
            .pop()
            .ok_or(ObjectValueError::GraphicsOperationSchemaError)?;
        match o {
            Object::LiteralString(s) => Ok(TextString::Text(s)),
            Object::HexString(s) => Ok(TextString::HexText(s)),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }
}

impl<'b> ConvertFromObject<'b> for TextStringOrNumber {
    fn convert_from_object(objects: &'b mut Vec<Object>) -> Result<Self, ObjectValueError> {
        let o = objects
            .pop()
            .ok_or(ObjectValueError::GraphicsOperationSchemaError)?;
        match o {
            Object::LiteralString(s) => Ok(TextStringOrNumber::TextString(TextString::Text(s))),
            Object::HexString(s) => Ok(TextStringOrNumber::TextString(TextString::HexText(s))),
            Object::Number(n) => Ok(TextStringOrNumber::Number(Length::new(n))),
            Object::Integer(v) => Ok(TextStringOrNumber::Number(Length::new(v as f32))),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }
}

impl<'b> ConvertFromObject<'b> for ColorArgsOrName {
    fn convert_from_object(objects: &'b mut Vec<Object>) -> Result<Self, ObjectValueError> {
        let o = objects
            .pop()
            .ok_or(ObjectValueError::GraphicsOperationSchemaError)?;
        if let Ok(name) = o.name() {
            if objects.is_empty() {
                Ok(ColorArgsOrName::Name((name, None)))
            } else {
                let args = ColorArgs::convert_from_object(objects)?;
                Ok(ColorArgsOrName::Name((name, Some(args))))
            }
        } else {
            objects.push(o);
            ColorArgs::convert_from_object(objects).map(ColorArgsOrName::Color)
        }
    }
}

impl<'b> ConvertFromObject<'b> for f32 {
    fn convert_from_object(objects: &'b mut Vec<Object>) -> Result<Self, ObjectValueError> {
        objects
            .pop()
            .ok_or(ObjectValueError::GraphicsOperationSchemaError)?
            .number()
    }
}

impl<'b, U> ConvertFromObject<'b> for Length<f32, U> {
    fn convert_from_object(objects: &'b mut Vec<Object>) -> Result<Self, ObjectValueError> {
        objects
            .pop()
            .ok_or(ObjectValueError::GraphicsOperationSchemaError)?
            .number()
            .map(|n| Length::new(n))
    }
}

/// Convert Object literal string to String
impl<'b> ConvertFromObject<'b> for String {
    fn convert_from_object(objects: &'b mut Vec<Object>) -> Result<Self, ObjectValueError> {
        objects
            .pop()
            .ok_or(ObjectValueError::GraphicsOperationSchemaError)?
            .as_str()
            .map(ToOwned::to_owned)
    }
}

/// Convert Object::Name to String
impl<'b> ConvertFromObject<'b> for NameOfDict {
    fn convert_from_object(objects: &'b mut Vec<Object>) -> Result<Self, ObjectValueError> {
        objects
            .pop()
            .ok_or(ObjectValueError::GraphicsOperationSchemaError)?
            .name()
            .map(NameOfDict)
    }
}

impl<'b> ConvertFromObject<'b> for NameOrDict {
    fn convert_from_object(objects: &'b mut Vec<Object>) -> Result<Self, ObjectValueError> {
        match objects
            .pop()
            .ok_or(ObjectValueError::GraphicsOperationSchemaError)?
        {
            Object::Name(name) => Ok(NameOrDict::Name(name)),
            Object::Dictionary(dict) => Ok(NameOrDict::Dict(dict)),
            _ => Err(ObjectValueError::GraphicsOperationSchemaError),
        }
    }
}

impl<'b, U> ConvertFromObject<'b> for Point2D<f32, U> {
    fn convert_from_object(objects: &'b mut Vec<Object>) -> Result<Self, ObjectValueError> {
        let y = objects
            .pop()
            .ok_or(ObjectValueError::GraphicsOperationSchemaError)?
            .number()?;
        let x = objects
            .pop()
            .ok_or(ObjectValueError::GraphicsOperationSchemaError)?
            .number()?;
        Ok(Self::new(x, y))
    }
}

#[derive(Debug, PartialEq)]
enum ObjectOrOperator<'a> {
    Object(Object),
    Operator(&'a str),
}

/// Parses `Operation::PaintInlineImage` operation.
/// `input` start after `BI`, parses dictionary and image data, consumes EI.
fn inline_image<'a, E>() -> impl Parser<&'a [u8], InlineImage, E>
where
    E: ParserError<&'a [u8]>
        + FromExternalError<&'a [u8], ObjectValueError>
        + FromExternalError<&'a [u8], ParseIntError>
        + FromExternalError<&'a [u8], hex::FromHexError>
        + FromExternalError<&'a [u8], AnyWhatever>
        + AddContext<&'a [u8], &'static str>
        + 'static,
{
    seq! {(
        wsc_prefixed0(parser::dict_body()).context("dict_body"),
        _: wsc0(), _: b"ID".as_slice(), _: alt((eol3(), any.void())), // after ID should has one single whitespace, but some invalid pdf use \r\n
        alt((
            repeat_till(1.., any, (one_of(b" \n"), b"EI".as_slice(), whitespace())).map(|(o, _)| o).context("inline image end with '[spaceOrNewLine]EI[wsc]'"),
            repeat_till(1.., any, (b"EI".as_slice(), whitespace())).map(|(o, _)| o).context("inline image end with 'EI[wsc]'"),
        ))
    )}
    .try_map(|(d, data): (Dictionary, Vec<u8>)| {
        InlineStream::new(d, &data).decode_image()
    })
}

pub fn parse_operations<'a, E>(buf: &mut &'a [u8]) -> PResult<Vec<Operation>, E>
where
    E: ParserError<&'a [u8]>
        + FromExternalError<&'a [u8], ObjectValueError>
        + FromExternalError<&'a [u8], ParseIntError>
        + FromExternalError<&'a [u8], hex::FromHexError>
        + FromExternalError<&'a [u8], Utf8Error>
        + FromExternalError<&'a [u8], AnyWhatever>
        + AddContext<&'a [u8], &'static str>
        + 'static,
{
    let operator = take_till(1.., b" \t\n\r%[<(/".as_slice())
        .try_map(|buf| Ok::<_, Utf8Error>(ObjectOrOperator::Operator(from_utf8(buf)?)))
        .context("operator");
    let mut object_or_operator = alt((
        parser::object_inside_page_stream()
            .map(ObjectOrOperator::Object)
            .context("operands"),
        operator,
    ));
    let mut operands = Vec::with_capacity(8);
    let mut r = vec![];
    loop {
        wsc0().parse_next(buf)?;
        if buf.is_empty() {
            if !operands.is_empty() {
                warn!("Not enough operands for operation");
            }
            return Ok(r);
        }
        let oo = object_or_operator.parse_next(buf).map_err(|e| {
            dbg!(&buf);
            e
        })?;
        match oo {
            ObjectOrOperator::Object(o) => operands.push(o),
            ObjectOrOperator::Operator(op) => {
                let opt_op = create_operation(op, &mut operands).unwrap_or_else(|e| {
                    // possible because not enough operands
                    warn!("Invalid operation '{}': {:?}", op, e);
                    None
                });
                debug!("Operation: {:?}", opt_op);
                match opt_op {
                    Some(
                        Operation::BeginCompatibilitySection | Operation::EndCompatibilitySection,
                    ) => {}
                    Some(Operation::BeginInlineImage) => {
                        let inline_image = inline_image()
                            .map(Operation::PaintInlineImage)
                            .parse_next(buf)?;
                        r.push(inline_image);
                    }
                    Some(op) => r.push(op),
                    None => {
                        warn!(
                            "Unknown operation: '{:?}', possible broken page content, abort parsing",
                            op
                        );
                        return Ok(r);
                    }
                }
                // Some pdf files has bug that has extra operands
                operands.clear();
            }
        }
    }
}

#[cfg(test)]
mod tests;
