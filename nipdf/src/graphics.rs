use crate::{
    ObjectValueError,
    graphics::trans::TextToUserSpace,
    object::{
        Array, Dictionary, InlineImage, InlineStream, Object, ObjectDiscriminants,
        ObjectWithResolver, RuntimeObjectId, Stream, TextString, TextStringOrNumber,
    },
    parser::{self, eol3, whitespace, wsc_prefixed0, wsc0},
};
use euclid::{Length, Point2D, Transform2D};
use log::{error, warn};
use nipdf_macro::{OperationParser, TryFromIntObject, TryFromNameObject, pdf_object};
use prescript::{Name, sname};
use snafu::{Report, ensure_whatever, whatever};
use std::{num::ParseIntError, str::Utf8Error};
use winnow::{
    Parser,
    combinator::{alt, repeat_till},
    error::{AddContext, ErrMode, FromExternalError, ParserError},
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

impl<S, T> TryFrom<ObjectWithResolver<'_, '_>> for Transform2D<f32, S, T> {
    type Error = ObjectValueError;

    fn try_from(obj: ObjectWithResolver<'_, '_>) -> Result<Self, Self::Error> {
        let arr = obj.into_schema_array()?;
        ensure_whatever!(
            arr.len() == 6,
            "expected array with 6 elements, but got {}",
            arr.len()
        );
        Ok(Self::new(
            arr.required_object(0)?.number()?,
            arr.required_object(1)?.number()?,
            arr.required_object(2)?.number()?,
            arr.required_object(3)?.number()?,
            arr.required_object(4)?.number()?,
            arr.required_object(5)?.number()?,
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

impl TryFrom<ObjectWithResolver<'_, '_>> for ColorSpaceArgs {
    type Error = ObjectValueError;

    fn try_from(obj: ObjectWithResolver<'_, '_>) -> Result<Self, Self::Error> {
        match obj.obj {
            Object::Name(name) => Ok(Self::Name(name.clone())),
            Object::Array(arr) => Ok(Self::Array(arr.clone())),
            Object::Reference(id) => Ok(Self::Ref(id.into())),
            _ => {
                error!("Can not parse ColorSpaceArgs from {:?}", obj.obj);
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

impl TryFrom<ObjectWithResolver<'_, '_>> for ColorArgs {
    type Error = ObjectValueError;

    fn try_from(obj: ObjectWithResolver<'_, '_>) -> Result<Self, Self::Error> {
        let arr = obj.into_schema_array()?;
        Ok(Self(match arr.len() {
            1 => vec![arr.required_object(0)?.number()?],
            3 => vec![
                arr.required_object(0)?.number()?,
                arr.required_object(1)?.number()?,
                arr.required_object(2)?.number()?,
            ],
            4 => vec![
                arr.required_object(0)?.number()?,
                arr.required_object(1)?.number()?,
                arr.required_object(2)?.number()?,
                arr.required_object(3)?.number()?,
            ],
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

impl<'a, 'b> TryFrom<ObjectWithResolver<'a, 'b>> for NameOrDictByRef<'b> {
    type Error = ObjectValueError;

    fn try_from(o: ObjectWithResolver<'a, 'b>) -> Result<Self, Self::Error> {
        match o.obj {
            Object::Name(name) => Ok(NameOrDictByRef::Name(name)),
            Object::Dictionary(dict) => Ok(NameOrDictByRef::Dict(dict)),
            Object::Stream(stream) => Ok(NameOrDictByRef::Dict(stream.as_dict())),
            _ => whatever!("Expect Name, Dictionary or Stream, but got {:?}", o.obj),
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

impl<'a, 'b> TryFrom<ObjectWithResolver<'a, 'b>> for NameOrStream<'b> {
    type Error = ObjectValueError;

    fn try_from(obj: ObjectWithResolver<'a, 'b>) -> Result<Self, Self::Error> {
        match obj.obj {
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
    #[op_tag(b"w")]
    SetLineWidth(f32),
    #[op_tag(b"J")]
    SetLineCap(LineCapStyle),
    #[op_tag(b"j")]
    SetLineJoin(LineJoinStyle),
    #[op_tag(b"M")]
    SetMiterLimit(f32),
    #[op_tag(b"d")]
    SetDashPattern(Vec<f32>, f32),
    #[op_tag(b"ri")]
    SetRenderIntent(RenderingIntent),
    #[op_tag(b"i")]
    SetFlatness(f32),
    #[op_tag(b"gs")]
    SetGraphicsStateParameters(NameOfDict),

    // Special Graphics State Operations
    #[op_tag(b"q")]
    SaveGraphicsState,
    #[op_tag(b"Q")]
    RestoreGraphicsState,
    #[op_tag(b"cm")]
    ModifyCTM(UserToUserSpace),

    // Path Construction Operations
    #[op_tag(b"m")]
    MoveToNext(Point),
    #[op_tag(b"l")]
    LineToNext(Point),
    #[op_tag(b"c")]
    AppendBezierCurve(Point, Point, Point),
    #[op_tag(b"v")]
    AppendBezierCurve2(Point, Point),
    #[op_tag(b"y")]
    AppendBezierCurve1(Point, Point),
    #[op_tag(b"h")]
    ClosePath,
    #[op_tag(b"re")]
    AppendRectangle(Point, f32, f32),

    // Path Painting Operations
    #[op_tag(b"S")]
    Stroke,
    #[op_tag(b"s")]
    CloseAndStroke,
    #[op_tag(b"f")]
    FillNonZero,
    #[op_tag(b"F")]
    FillNonZeroDeprecated,
    #[op_tag(b"f*")]
    FillEvenOdd,
    #[op_tag(b"B")]
    FillAndStrokeNonZero,
    #[op_tag(b"B*")]
    FillAndStrokeEvenOdd,
    #[op_tag(b"b")]
    CloseFillAndStrokeNonZero,
    #[op_tag(b"b*")]
    CloseFillAndStrokeEvenOdd,
    #[op_tag(b"n")]
    EndPath,

    // Clipping Path Operations
    #[op_tag(b"W")]
    ClipNonZero,
    #[op_tag(b"W*")]
    ClipEvenOdd,

    // Text Object Operations
    #[op_tag(b"BT")]
    BeginText,
    #[op_tag(b"ET")]
    EndText,

    // Text State Operations
    #[op_tag(b"Tc")]
    SetCharacterSpacing(Length<f32, TextSpace>),
    #[op_tag(b"Tw")]
    SetWordSpacing(Length<f32, TextSpace>),
    #[op_tag(b"Tz")]
    SetHorizontalScaling(f32),
    #[op_tag(b"TL")]
    SetLeading(f32),
    #[op_tag(b"Tf")]
    SetFont(NameOfDict, f32),
    #[op_tag(b"Tr")]
    SetTextRenderingMode(TextRenderingMode),
    #[op_tag(b"Ts")]
    SetTextRise(f32),

    // Text Positioning Operations
    #[op_tag(b"Td")]
    MoveTextPosition(TextPoint),
    #[op_tag(b"TD")]
    MoveTextPositionAndSetLeading(TextPoint),
    #[op_tag(b"Tm")]
    SetTextMatrix(TextToUserSpace),
    #[op_tag(b"T*")]
    MoveToStartOfNextLine,

    // Text Showing Operations
    #[op_tag(b"Tj")]
    ShowText(TextString),
    #[op_tag(b"TJ")]
    ShowTexts(Vec<TextStringOrNumber>),
    #[op_tag(b"'")]
    MoveToNextLineAndShowText(TextString),
    #[op_tag(b"\"")]
    SetSpacingMoveToNextLineAndShowText(Length<f32, TextSpace>, Length<f32, TextSpace>, TextString),

    // Type 3 Font Operations
    #[op_tag(b"d0")]
    SetGlyphWidth(Point),
    #[op_tag(b"d1")]
    SetGlyphWidthAndBoundingBox(Point, Point, Point),

    // Color Operations
    #[op_tag(b"CS")]
    SetStrokeColorSpace(ColorSpaceArgs),
    #[op_tag(b"cs")]
    SetFillColorSpace(ColorSpaceArgs),
    #[op_tag(b"SC")]
    SetStrokeColor(ColorArgs),
    #[op_tag(b"SCN")]
    SetStrokeColorOrWithPattern(ColorArgsOrName),
    #[op_tag(b"sc")]
    SetFillColor(ColorArgs),
    #[op_tag(b"scn")]
    SetFillColorOrWithPattern(ColorArgsOrName),
    #[op_tag(b"G")]
    SetStrokeGray([f32; 1]), // Should be Color::Gray
    #[op_tag(b"g")]
    SetFillGray([f32; 1]),   // Should be Color::Gray
    #[op_tag(b"RG")]
    SetStrokeRGB([f32; 3]), // Should be Color::Rgb
    #[op_tag(b"rg")]
    SetFillRGB([f32; 3]),   // Should be Color::Rgb
    #[op_tag(b"K")]
    SetStrokeCMYK([f32; 4]), // Should be Color::Cmyk
    #[op_tag(b"k")]
    SetFillCMYK([f32; 4]),   // Should be Color::Cmyk

    // Shading Operation
    #[op_tag(b"sh")]
    PaintShading(NameOfDict),

    // Inline Image Operations
    #[op_tag(b"BI")]
    BeginInlineImage,
    #[op_tag(b"ID")]
    BeginInlineImageData,
    #[op_tag(b"EI")]
    EndInlineImage,
    #[op_tag(b"paint-inline-image")]
    PaintInlineImage(InlineImage),

    // XObject Operation
    #[op_tag(b"Do")]
    PaintXObject(NameOfDict),

    // Marked Content Operations
    #[op_tag(b"MP")]
    DesignateMarkedContentPoint(NameOfDict),
    #[op_tag(b"DP")]
    DesignateMarkedContentPointWithProperties(NameOfDict, NameOrDict),
    #[op_tag(b"BMC")]
    BeginMarkedContent(NameOfDict),
    #[op_tag(b"BDC")]
    BeginMarkedContentWithProperties(NameOfDict, NameOrDict),
    #[op_tag(b"EMC")]
    EndMarkedContent,

    // Compatibility Operations
    #[op_tag(b"BX")]
    BeginCompatibilitySection,
    #[op_tag(b"EX")]
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
            _ => whatever!(
                "expected literal string or hex string, but got {}",
                ObjectDiscriminants::from(o)
            ),
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
            _ => whatever!(
                "expected literal string or hex string or number or integer, but got {}",
                ObjectDiscriminants::from(o)
            ),
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
    Operator(&'a [u8]),
}

/// Parses `Operation::PaintInlineImage` operation.
/// `input` start after `BI`, parses dictionary and image data, consumes EI.
fn inline_image<'a, E>() -> impl Parser<&'a [u8], InlineImage, ErrMode<E>>
where
    E: ParserError<&'a [u8]>
        + FromExternalError<&'a [u8], ObjectValueError>
        + FromExternalError<&'a [u8], ParseIntError>
        + FromExternalError<&'a [u8], hex::FromHexError>
        + FromExternalError<&'a [u8], ObjectValueError>
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

fn operation<'a, E>(buf: &mut &'a [u8]) -> impl FnMut() -> Option<Operation>
where
    E: ParserError<&'a [u8]>
        + FromExternalError<&'a [u8], ObjectValueError>
        + FromExternalError<&'a [u8], ParseIntError>
        + FromExternalError<&'a [u8], hex::FromHexError>
        + FromExternalError<&'a [u8], Utf8Error>
        + FromExternalError<&'a [u8], ObjectValueError>
        + AddContext<&'a [u8], &'static str>
        + std::fmt::Debug
        + 'static,
    prescript::ParserError: winnow::error::ErrorConvert<E>,
{
    move || {
        let mut object_or_operator = alt((
            parser::object_inside_page_stream::<_, prescript::ParserError>()
                .map(ObjectOrOperator::Object)
                .context("operands"),
            take_till(1.., b" \t\n\r%[<(/".as_slice())
                .map(ObjectOrOperator::Operator)
                .context("operator"),
        ));
        let mut operands = Vec::with_capacity(8);
        loop {
            wsc0::<_, E>().parse_next(buf).unwrap();
            if buf.is_empty() {
                return None;
            }
            let oo = object_or_operator.parse_next(buf);
            match oo {
                Ok(ObjectOrOperator::Object(o)) => operands.push(o),
                Ok(ObjectOrOperator::Operator(op)) => {
                    let opt_op = create_operation(op, &mut operands).unwrap_or_else(|e| {
                        // possible because not enough operands
                        let op = String::from_utf8_lossy(op);
                        warn!("Invalid operation '{}': {:?}", op, e);
                        None
                    });
                    match opt_op {
                        Some(Operation::BeginInlineImage) => {
                            match inline_image::<prescript::ParserError>()
                                .map(Operation::PaintInlineImage)
                                .parse_next(buf)
                            {
                                Ok(op) => {
                                    if !operands.is_empty() {
                                        warn!("object not all consumed");
                                    }
                                    return Some(op);
                                }
                                Err(e) => {
                                    warn!("Error parsing inline image: {:?}", e);
                                }
                            };
                        }
                        Some(r) => {
                            if !operands.is_empty() {
                                warn!("object not all consumed");
                            }
                            return Some(r);
                        }
                        None => {
                            warn!("Unknown page operation: '{:?}', try recover romains", op);
                        }
                    }
                    // Some pdf files has bug that has extra operands
                    operands.clear();
                }
                Err(e @ ErrMode::Incomplete(_)) => {
                    unreachable!("{:?}", e);
                }
                Err(ErrMode::Backtrack(e) | ErrMode::Cut(e)) => {
                    warn!("Ignore operation parsing error: {}", Report::from_error(e));
                    operands.clear();
                }
            }
        }
    }
}

pub fn parse_operations<'a, E>(buf: &mut &'a [u8]) -> impl Iterator<Item = Operation>
where
    E: ParserError<&'a [u8]>
        + FromExternalError<&'a [u8], ObjectValueError>
        + FromExternalError<&'a [u8], ParseIntError>
        + FromExternalError<&'a [u8], hex::FromHexError>
        + FromExternalError<&'a [u8], Utf8Error>
        + FromExternalError<&'a [u8], ObjectValueError>
        + AddContext<&'a [u8], &'static str>
        + std::fmt::Debug
        + 'static,
    prescript::ParserError: winnow::error::ErrorConvert<E>,
{
    std::iter::from_fn(operation::<prescript::ParserError>(buf))
}

#[cfg(test)]
mod tests;
