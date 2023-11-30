use crate::{
    graphics::trans::TextToUserSpace,
    object::{Array, Dictionary, Object, ObjectValueError, Stream, TextString, TextStringOrNumber},
    parser::{parse_object, whitespace_or_comment, ws_prefixed, ParseError, ParseResult},
};
use euclid::Transform2D;
use log::error;
use nipdf_macro::{pdf_object, OperationParser, TryFromIntObject, TryFromNameObject};
use nom::{
    branch::alt,
    bytes::complete::is_not,
    combinator::map_res,
    error::{ErrorKind, FromExternalError, ParseError as NomParseError},
    Err, Parser,
};
use prescript::Name;
use std::num::NonZeroU32;

pub(crate) mod color_space;
mod pattern;
pub(crate) mod trans;
use self::trans::UserToUserSpace;
pub(crate) use pattern::*;

pub(crate) mod shading;

impl<S, T> TryFrom<&Object> for Transform2D<f32, S, T> {
    type Error = ObjectValueError;

    fn try_from(obj: &Object) -> Result<Self, Self::Error> {
        let arr = obj.arr()?;
        if arr.len() != 6 {
            return Err(ObjectValueError::UnexpectedType);
        }
        Ok(Self::new(
            arr[0].as_number()?,
            arr[1].as_number()?,
            arr[2].as_number()?,
            arr[3].as_number()?,
            arr[4].as_number()?,
            arr[5].as_number()?,
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl<U> From<Point> for euclid::Point2D<f32, U> {
    fn from(p: Point) -> Self {
        Self::new(p.x, p.y)
    }
}

impl Point {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn distance(self, b: Self) -> f32 {
        (self.x - b.x).hypot(self.y - b.y)
    }

    pub fn x_y_distance(self, b: Self) -> Self {
        Self {
            x: self.x - b.x,
            y: self.y - b.y,
        }
    }
}

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
            if let Ok(num) = o.as_number() {
                result.push(num);
            } else {
                todo!("color args: {:?}", o);
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
    Ref(NonZeroU32),
}

impl<'b> TryFrom<&'b Object> for ColorSpaceArgs {
    type Error = ObjectValueError;

    fn try_from(object: &'b Object) -> Result<Self, Self::Error> {
        match object {
            Object::Name(name) => Ok(Self::Name(name.clone())),
            Object::Array(arr) => Ok(Self::Array(arr.clone())),
            Object::Reference(id) => Ok(Self::Ref(id.id().id())),
            _ => {
                error!("Can not parse ColorSpaceArgs from {:?}", object);
                Err(ObjectValueError::GraphicsOperationSchemaError)
            }
        }
    }
}

impl<'b> ConvertFromObject<'b> for ColorSpaceArgs {
    fn convert_from_object(objects: &'b mut Vec<Object>) -> Result<Self, ObjectValueError> {
        let o = objects.pop().unwrap();
        ColorSpaceArgs::try_from(&o).map_err(|_| ObjectValueError::GraphicsOperationSchemaError)
    }
}

#[pdf_object(())]
trait ICCStreamDictTrait {
    fn n(&self) -> u32;
    #[try_from]
    fn alternate(&self) -> Option<ColorSpaceArgs>;
}

impl<'b, const N: usize> ConvertFromObject<'b> for [f32; N] {
    fn convert_from_object(objects: &'b mut Vec<Object>) -> Result<Self, ObjectValueError> {
        let mut result = [0.0; N];
        for i in 0..N {
            result[N - 1 - i] = objects.pop().unwrap().as_number()?;
        }
        Ok(result)
    }
}

impl TryFrom<&Object> for ColorArgs {
    type Error = ObjectValueError;

    fn try_from(obj: &Object) -> Result<Self, Self::Error> {
        Ok(Self(match obj {
            Object::Array(arr) => match arr.len() {
                1 => vec![arr[0].as_number()?],
                3 => vec![
                    arr[0].as_number()?,
                    arr[1].as_number()?,
                    arr[2].as_number()?,
                ],
                4 => vec![
                    arr[0].as_number()?,
                    arr[1].as_number()?,
                    arr[2].as_number()?,
                    arr[3].as_number()?,
                ],
                _ => return Err(ObjectValueError::GraphicsOperationSchemaError),
            },
            _ => return Err(ObjectValueError::GraphicsOperationSchemaError),
        }))
    }
}

impl<'b, S, T> ConvertFromObject<'b> for Transform2D<f32, S, T> {
    fn convert_from_object(objects: &'b mut Vec<Object>) -> Result<Self, ObjectValueError> {
        let f = objects.pop().unwrap().as_number()?;
        let e = objects.pop().unwrap().as_number()?;
        let d = objects.pop().unwrap().as_number()?;
        let c = objects.pop().unwrap().as_number()?;
        let b = objects.pop().unwrap().as_number()?;
        let a = objects.pop().unwrap().as_number()?;
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
    SetCharacterSpacing(f32),
    #[op_tag("Tw")]
    SetWordSpacing(f32),
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
    MoveTextPosition(Point),
    #[op_tag("TD")]
    MoveTextPositionAndSetLeading(Point),
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
    MoveToNextLineAndShowText(String),
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
        let mut arr: Vec<_> = objects.pop().unwrap().into_arr()?.iter().cloned().collect();
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
        let o = objects.pop().unwrap();
        match o {
            Object::LiteralString(s) => Ok(TextString::Text(s)),
            Object::HexString(s) => Ok(TextString::HexText(s)),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }
}

impl<'b> ConvertFromObject<'b> for TextStringOrNumber {
    fn convert_from_object(objects: &'b mut Vec<Object>) -> Result<Self, ObjectValueError> {
        let o = objects.pop().unwrap();
        match o {
            Object::LiteralString(s) => Ok(TextStringOrNumber::TextString(TextString::Text(s))),
            Object::HexString(s) => Ok(TextStringOrNumber::TextString(TextString::HexText(s))),
            Object::Number(n) => Ok(TextStringOrNumber::Number(n)),
            Object::Integer(v) => Ok(TextStringOrNumber::Number(v as f32)),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }
}

impl<'b> ConvertFromObject<'b> for ColorArgsOrName {
    fn convert_from_object(objects: &'b mut Vec<Object>) -> Result<Self, ObjectValueError> {
        let o = objects.pop().unwrap();
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
        let o = objects.pop().unwrap();
        o.as_number()
    }
}

/// Convert Object literal string to String
impl<'b> ConvertFromObject<'b> for String {
    fn convert_from_object(objects: &'b mut Vec<Object>) -> Result<Self, ObjectValueError> {
        let o = objects.pop().unwrap();
        o.as_text_string().map(|s| s.to_owned())
    }
}

/// Convert Object::Name to String
impl<'b> ConvertFromObject<'b> for NameOfDict {
    fn convert_from_object(objects: &'b mut Vec<Object>) -> Result<Self, ObjectValueError> {
        let o = objects.pop().unwrap();
        o.name().map(NameOfDict)
    }
}

impl<'b> ConvertFromObject<'b> for NameOrDict {
    fn convert_from_object(objects: &'b mut Vec<Object>) -> Result<Self, ObjectValueError> {
        match objects.pop().unwrap() {
            Object::Name(name) => Ok(NameOrDict::Name(name)),
            Object::Dictionary(dict) => Ok(NameOrDict::Dict(dict)),
            _ => Err(ObjectValueError::GraphicsOperationSchemaError),
        }
    }
}

impl<'b> ConvertFromObject<'b> for Point {
    fn convert_from_object(objects: &'b mut Vec<Object>) -> Result<Self, ObjectValueError> {
        let y = objects.pop().unwrap().as_number()?;
        let x = objects.pop().unwrap().as_number()?;
        Ok(Self { x, y })
    }
}

#[derive(Debug, PartialEq)]
enum ObjectOrOperator<'a> {
    Object(Object),
    Operator(&'a str),
}

fn parse_operator(input: &[u8]) -> ParseResult<ObjectOrOperator> {
    let p = is_not(b" \t\n\r%[<(/".as_slice());
    map_res(p, |op| {
        let op = unsafe { std::str::from_utf8_unchecked(op) };
        Ok::<_, ParseError>(ObjectOrOperator::Operator(op))
    })(input)
}

fn parse_object_or_operator(input: &[u8]) -> ParseResult<ObjectOrOperator> {
    alt((parse_object.map(ObjectOrOperator::Object), parse_operator))(input)
}

pub fn parse_operations2(input: &[u8]) -> ParseResult<Vec<Operation>> {
    let mut operands = Vec::with_capacity(8);
    parse_operations(&mut operands, input)
}

pub fn parse_operations<'a>(
    operands: &mut Vec<Object>,
    mut input: &'a [u8],
) -> ParseResult<'a, Vec<Operation>> {
    let mut ignore_parse_error = false;
    let mut r = vec![];
    loop {
        let vr = ws_prefixed(parse_object_or_operator)(input);
        match vr {
            Err(Err::Error(_)) => break,
            Err(e) => return Err(e),
            Ok((remains, vr)) => {
                input = remains;
                match vr {
                    ObjectOrOperator::Object(o) => {
                        operands.push(o);
                    }
                    ObjectOrOperator::Operator(op) => {
                        let (input, opt_op) = (
                            input,
                            create_operation(op, operands).map_err(|e| {
                                nom::Err::Error(ParseError::from_external_error(
                                    input,
                                    ErrorKind::Fail,
                                    e,
                                ))
                            })?,
                        );
                        match opt_op {
                            Some(Operation::BeginCompatibilitySection) => ignore_parse_error = true,
                            Some(Operation::EndCompatibilitySection) => ignore_parse_error = false,
                            Some(op) => r.push(op),
                            None => {
                                if ignore_parse_error {
                                    operands.clear();
                                } else {
                                    error!("Unknown operation: {:?}", op);
                                    return Err(nom::Err::Error(ParseError::from_error_kind(
                                        input,
                                        ErrorKind::Tag,
                                    )));
                                }
                            }
                        }
                        assert!(operands.is_empty());
                    }
                }
            }
        }
        (input, _) = whitespace_or_comment(input)?;
    }

    Ok((input, r))
}

#[cfg(test)]
mod tests;
