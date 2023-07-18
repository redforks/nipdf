use std::collections::HashSet;

use lazy_static::lazy_static;
use nom::{branch::alt, bytes::complete::is_not, combinator::map_res, multi::many0, Parser};

use crate::{
    object::{Object, ObjectValueError, TextStringOrNumber},
    parser::{parse_object, ws_prefixed, ParseError, ParseResult},
};
use pdf2docx_macro::{ConvertFromIntObject, ConvertFromNameObject, OperationParser};

#[derive(Debug, Clone, PartialEq)]
pub struct TransformMatrix {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    x: f32,
    y: f32,
}

impl Point {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, ConvertFromIntObject)]
pub enum LineCapStyle {
    Butt = 0,
    Round = 1,
    Square = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, ConvertFromIntObject)]
pub enum LineJoinStyle {
    Miter = 0,
    Round = 1,
    Bevel = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, ConvertFromNameObject)]
pub enum RenderingIntent {
    AbsoluteColorimetric,
    RelativeColorimetric,
    Saturation,
    Perceptual,
}

#[derive(Debug, Clone, Copy, PartialEq, ConvertFromIntObject)]
pub enum SetTextRenderingMode {
    Fill = 0,
    Stroke = 1,
    FillAndStroke = 2,
    Invisible = 3,
    FillAndClip = 4,
    StrokeAndClip = 5,
    FillStrokeAndClip = 6,
    Clip = 7,
}

/// Color for different color space
#[derive(Debug, Clone, PartialEq)]
pub enum Color {
    /// DeviceGray, CalGray, Indexed
    Gray(f32),
    /// DeviceRGB, CalRGB, Lab
    Rgb(f32, f32, f32),
    /// DeviceCMYK
    Cmyk(f32, f32, f32, f32),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ColorOrWithPattern {
    Color(Color),
    WithPattern(Color, String),
}

/// Alias of Vec<f32> for easier parse by [[graphics_operation_parser]] macro
pub type VecF32 = Vec<f32>;
pub type VecTextStringOrNumber = Vec<TextStringOrNumber>;

#[derive(Debug, Clone, PartialEq)]
pub struct NameOfDict(pub String);

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
    SetDashPattern(VecF32, f32),
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
    ModifyCTM(TransformMatrix),

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
    SetFont(String, f32),
    #[op_tag("Tr")]
    SetTextRenderingMode(SetTextRenderingMode),
    #[op_tag("Ts")]
    SetTextRise(f32),

    // Text Positioning Operations
    #[op_tag("Td")]
    MoveTextPosition(Point),
    #[op_tag("TD")]
    MoveTextPositionAndSetLeading(Point),
    #[op_tag("Tm")]
    SetTextMatrix(TransformMatrix),
    #[op_tag("T*")]
    MoveToStartOfNextLine,

    // Text Showing Operations
    #[op_tag("Tj")]
    ShowText(String),
    #[op_tag("TJ")]
    ShowTexts(VecTextStringOrNumber),
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
    SetStrokeColorSpace(NameOfDict),
    #[op_tag("cs")]
    SetFillColorSpace(NameOfDict),
    #[op_tag("SC")]
    SetStrokeColor(Color),
    #[op_tag("SCN")]
    SetStrokeColorOrWithPattern(ColorOrWithPattern),
    #[op_tag("sc")]
    SetFillColor(Color),
    #[op_tag("scn")]
    SetFillColorOrWithPattern(ColorOrWithPattern),
    #[op_tag("G")]
    SetStrokeGray(Color), // Should be Color::Gray
    #[op_tag("g")]
    SetFillGray(Color),   // Should be Color::Gray
    #[op_tag("RG")]
    SetStrokeRGB(Color), // Should be Color::Rgb
    #[op_tag("rg")]
    SetFillRGB(Color),   // Should be Color::Rgb
    #[op_tag("K")]
    SetStrokeCMYK(Color), // Should be Color::Cmyk
    #[op_tag("k")]
    SetFillCMYK(Color),   // Should be Color::Cmyk
}

trait ConvertFromObject<'a, 'b>
where
    Self: Sized,
{
    fn convert_from_object(objects: &'b mut Vec<Object<'a>>) -> Result<Self, ObjectValueError>;
}

impl<'a, 'b, T: ConvertFromObject<'a, 'b>> ConvertFromObject<'a, 'b> for Box<T> {
    fn convert_from_object(objects: &'b mut Vec<Object<'a>>) -> Result<Self, ObjectValueError> {
        Ok(Box::new(T::convert_from_object(objects)?))
    }
}

impl<'a, 'b, T: for<'c, 'd> ConvertFromObject<'c, 'd>> ConvertFromObject<'a, 'b> for Vec<T> {
    fn convert_from_object(objects: &'b mut Vec<Object<'a>>) -> Result<Self, ObjectValueError> {
        let mut arr = objects.pop().unwrap().into_arr()?;
        let mut result = Vec::new();
        while !arr.is_empty() {
            result.push(T::convert_from_object(&mut arr)?);
        }
        result.reverse();
        Ok(result)
    }
}

impl<'a, 'b> ConvertFromObject<'a, 'b> for TextStringOrNumber {
    fn convert_from_object(objects: &'b mut Vec<Object<'a>>) -> Result<Self, ObjectValueError> {
        let o = objects.pop().unwrap();
        o.as_text_string_or_number()
    }
}

impl<'a, 'b> ConvertFromObject<'a, 'b> for Color {
    fn convert_from_object(objects: &'b mut Vec<Object<'a>>) -> Result<Self, ObjectValueError> {
        let mut colors = Vec::with_capacity(4);
        while let Some(o) = objects.pop() {
            if let Ok(num) = o.as_number() {
                colors.push(num);
                if colors.len() == 4 {
                    break;
                }
            } else {
                objects.push(o);
                break;
            }
        }

        match colors.len() {
            1 => Ok(Color::Gray(colors[0])),
            3 => Ok(Color::Rgb(colors[2], colors[1], colors[0])),
            4 => Ok(Color::Cmyk(colors[3], colors[2], colors[1], colors[0])),
            _ => Err(ObjectValueError::GraphicsOperationSchemaError),
        }
    }
}

/// If last element in objects is Name, return `ColorOrWithPattern::WithPattern`
/// Otherwise, return `ColorOrWithPattern::Color`
impl<'a, 'b> ConvertFromObject<'a, 'b> for ColorOrWithPattern {
    fn convert_from_object(objects: &'b mut Vec<Object<'a>>) -> Result<Self, ObjectValueError> {
        let o = objects.pop().unwrap();
        if let Ok(name) = o.as_name() {
            let color = Color::convert_from_object(objects)?;
            Ok(ColorOrWithPattern::WithPattern(color, name.to_owned()))
        } else {
            objects.push(o);
            Color::convert_from_object(objects).map(ColorOrWithPattern::Color)
        }
    }
}

impl<'a, 'b> ConvertFromObject<'a, 'b> for f32 {
    fn convert_from_object(objects: &'b mut Vec<Object<'a>>) -> Result<Self, ObjectValueError> {
        let o = objects.pop().unwrap();
        o.as_number()
    }
}

/// Convert Object literal string to String
impl<'a, 'b> ConvertFromObject<'a, 'b> for String {
    fn convert_from_object(objects: &'b mut Vec<Object<'a>>) -> Result<Self, ObjectValueError> {
        let o = objects.pop().unwrap();
        o.as_text_string()
    }
}

/// Convert Object::Name to String
impl<'a, 'b> ConvertFromObject<'a, 'b> for NameOfDict {
    fn convert_from_object(objects: &'b mut Vec<Object<'a>>) -> Result<Self, ObjectValueError> {
        let o = objects.pop().unwrap();
        o.as_name().map(|s| NameOfDict(s.to_string()))
    }
}

impl<'a, 'b> ConvertFromObject<'a, 'b> for Point {
    fn convert_from_object(objects: &'b mut Vec<Object<'a>>) -> Result<Self, ObjectValueError> {
        let y = objects.pop().unwrap().as_number()?;
        let x = objects.pop().unwrap().as_number()?;
        Ok(Self { x, y })
    }
}

impl<'a, 'b> ConvertFromObject<'a, 'b> for TransformMatrix {
    fn convert_from_object(objects: &'b mut Vec<Object<'a>>) -> Result<Self, ObjectValueError> {
        let f = objects.pop().unwrap().as_number()?;
        let e = objects.pop().unwrap().as_number()?;
        let d = objects.pop().unwrap().as_number()?;
        let c = objects.pop().unwrap().as_number()?;
        let b = objects.pop().unwrap().as_number()?;
        let a = objects.pop().unwrap().as_number()?;
        Ok(Self { a, b, c, d, e, f })
    }
}

#[derive(Debug, PartialEq)]
enum ObjectOrOperator<'a> {
    Object(Object<'a>),
    Operator(&'a str),
}

lazy_static! {
    static ref OPERATORS: HashSet<&'static str> = [
        // General graphics state
        "w", "J", "j", "M", "d", "ri", "i", "gs",
        // Special graphics state
        "q", "Q", "cm",
        // Path construction
        "m", "l", "c", "v", "y", "h", "re",
        // Path Painting
        "S", "s", "f", "F", "f*", "B", "B*", "b", "b*", "n",
        // Clipping paths
        "W", "W*",
        // Text objects
        "BT", "ET",
        // Text state
        "Tc", "Tw", "Tz", "TL", "Tf", "Tr", "Ts",
        // Text positioning
        "Td", "TD", "Tm", "T*",
        // Text showing
        "Tj", "TJ","'", "\"",
        // Type 3 font
        "d0", "d1",
        // Color
        "CS", "cs", "SC", "SCN", "sc", "scn", "G", "g", "RG", "rg", "K", "k",
        // Shading patterns
        "sh",
        // Inline images
        "BI", "ID", "EI",
        // XObjects
        "Do",
        // Marked content
        "MP", "DP", "BMC", "BDC", "EMC",
        // Compatibility
        "BX", "EX",
    ].iter().copied().collect();
}

fn parse_operator(input: &[u8]) -> ParseResult<ObjectOrOperator> {
    let p = is_not(b" \t\n\r%".as_slice());
    map_res(p, |op| {
        let op = unsafe { std::str::from_utf8_unchecked(op) };
        if OPERATORS.contains(op) {
            Ok(ObjectOrOperator::Operator(op))
        } else {
            Err(ParseError::UnknownGraphicOperator(op.to_owned()))
        }
    })(input)
}

fn parse_object_or_operator(input: &[u8]) -> ParseResult<ObjectOrOperator> {
    alt((parse_object.map(ObjectOrOperator::Object), parse_operator))(input)
}

fn parse_operation(mut input: &[u8]) -> ParseResult<Operation> {
    let mut operands = Vec::with_capacity(8);
    loop {
        let vr = ws_prefixed(parse_object_or_operator)(input)?;
        match vr {
            (remains, ObjectOrOperator::Object(o)) => {
                input = remains;
                operands.push(o);
            }
            (remains, ObjectOrOperator::Operator(op)) => {
                input = remains;
                let r = (input, create_operation(op, &mut operands)?);
                assert!(operands.is_empty());
                return Ok(r);
            }
        }
    }
}

pub fn parse_operations(input: &[u8]) -> ParseResult<Vec<Operation>> {
    many0(parse_operation)(input)
}

#[cfg(test)]
mod tests;
