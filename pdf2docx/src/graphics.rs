use std::collections::{HashMap, HashSet};

use ahash::RandomState;
use arrayvec::ArrayVec;
use educe::Educe;
use lazy_static::lazy_static;
use nom::{
    branch::alt,
    bytes::complete::is_not,
    combinator::map_res,
    error::{ErrorKind, FromExternalError, ParseError as NomParseError},
    multi::many0,
    Parser,
};

use crate::{
    file::{GraphicsStateParameterDict, ObjectResolver, Rectangle},
    function::{default_domain, Domain},
    object::{
        ColorSpace, Dictionary, Name, Object, ObjectValueError, SchemaDict, TextStringOrNumber,
    },
    parser::{parse_object, ws_prefixed, ws_terminated, ParseError, ParseResult},
};
use pdf2docx_macro::{pdf_object, OperationParser, TryFromIntObject, TryFromNameObject};

#[derive(Debug, Clone, Copy, PartialEq, Educe)]
#[educe(Default)]
pub struct TransformMatrix {
    #[educe(Default = 1.0)]
    pub sx: f32,
    pub kx: f32,
    pub ky: f32,
    #[educe(Default = 1.0)]
    pub sy: f32,
    pub tx: f32,
    pub ty: f32,
}

impl TransformMatrix {
    pub fn identity() -> Self {
        Self::default()
    }
}

impl From<TransformMatrix> for tiny_skia::Transform {
    fn from(m: TransformMatrix) -> Self {
        Self::from_row(m.sx, m.ky, m.kx, m.sy, m.tx, m.ty)
    }
}

/// Create TransformMatrix from Object::Array
impl<'a> TryFrom<&Object<'a>> for TransformMatrix {
    type Error = ObjectValueError;

    fn try_from(obj: &Object) -> Result<Self, Self::Error> {
        let arr = obj.as_arr()?;
        if arr.len() != 6 {
            return Err(ObjectValueError::UnexpectedType);
        }
        Ok(TransformMatrix {
            sx: arr[0].as_number()?,
            kx: arr[1].as_number()?,
            ky: arr[2].as_number()?,
            sy: arr[3].as_number()?,
            tx: arr[4].as_number()?,
            ty: arr[5].as_number()?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
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
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Color {
    /// DeviceGray, CalGray, Indexed
    Gray(f32),
    /// DeviceRGB, CalRGB, Lab
    Rgb(f32, f32, f32),
    /// DeviceCMYK
    Cmyk(f32, f32, f32, f32),
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, TryFromIntObject)]
pub(crate) enum PatternType {
    Tiling = 1,
    Shading = 2,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, TryFromIntObject)]
pub(crate) enum ShadingType {
    Function = 1,
    Axial = 2,
    Radial = 3,
    FreeForm = 4,
    LatticeForm = 5,
    CoonsPatchMesh = 6,
    TensorProductPatchMesh = 7,
}

#[pdf_object(Some("Pattern"))]
pub(crate) trait PatternDictTrait {
    #[try_from]
    fn pattern_type(&self) -> PatternType;

    /// don't call these methods if pattern_type() != PatternType::Shading
    #[nested]
    fn shading(&self) -> ShadingDict<'a, 'b>;
    #[try_from]
    #[or_default]
    fn matrix(&self) -> TransformMatrix;
    #[nested]
    fn ext_g_state() -> HashMap<String, GraphicsStateParameterDict<'a, 'b>>;
}

#[pdf_object(())]
pub(crate) trait ShadingDictTrait {
    #[try_from]
    fn shading_type(&self) -> ShadingType;

    #[try_from]
    fn color_space(&self) -> ColorSpace;

    #[try_from]
    fn b_box(&self) -> Option<Rectangle>;
    fn anti_alias(&self) -> Option<bool>;

    #[self_as]
    fn axial(&self) -> AxialShadingDict<'a, 'b>;
}

/// Return type of `AxialShadingDict::extend()`
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct AxialExtend(bool, bool);

impl AxialExtend {
    pub fn begin(&self) -> bool {
        self.0
    }

    pub fn end(&self) -> bool {
        self.1
    }
}

impl<'a> TryFrom<&Object<'a>> for AxialExtend {
    type Error = ObjectValueError;

    fn try_from(obj: &Object) -> Result<Self, Self::Error> {
        let arr = obj.as_arr()?;
        if arr.len() != 2 {
            return Err(ObjectValueError::UnexpectedType);
        }
        Ok(Self(arr[0].as_bool()?, arr[1].as_bool()?))
    }
}

#[pdf_object(2i32)]
#[type_field("ShadingType")]
pub(crate) trait AxialShadingDictTrait {
    #[try_from]
    fn coords(&self) -> Rectangle;

    #[try_from]
    #[default_fn(default_domain)]
    fn domain(&self) -> Domain;

    #[try_from]
    #[or_default]
    fn extend(&self) -> AxialExtend;
}

#[derive(Debug, Clone, PartialEq)]
pub enum ColorOrName {
    Color(Color),
    Name(String),
}

impl<'a, 'b> ConvertFromObject<'a, 'b> for TransformMatrix {
    fn convert_from_object(objects: &'b mut Vec<Object<'a>>) -> Result<Self, ObjectValueError> {
        let f = objects.pop().unwrap().as_number()?;
        let e = objects.pop().unwrap().as_number()?;
        let d = objects.pop().unwrap().as_number()?;
        let c = objects.pop().unwrap().as_number()?;
        let b = objects.pop().unwrap().as_number()?;
        let a = objects.pop().unwrap().as_number()?;
        Ok(Self {
            sx: a,
            kx: b,
            ky: c,
            sy: d,
            tx: e,
            ty: f,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NameOfDict(pub String);

#[derive(Debug, Clone, PartialEq)]
pub enum NameOrDict<'a> {
    Name(Name<'a>),
    Dict(Dictionary<'a>),
}

#[derive(Debug, Clone, PartialEq, OperationParser)]
#[rustfmt::skip]
pub enum Operation<'a> {
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
    SetFont(NameOfDict, f32),
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
    SetStrokeColorSpace(NameOfDict),
    #[op_tag("cs")]
    SetFillColorSpace(NameOfDict),
    #[op_tag("SC")]
    SetStrokeColor(Color),
    #[op_tag("SCN")]
    SetStrokeColorOrWithPattern(ColorOrName),
    #[op_tag("sc")]
    SetFillColor(Color),
    #[op_tag("scn")]
    SetFillColorOrWithPattern(ColorOrName),
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
    DesignateMarkedContentPointWithProperties(NameOfDict, NameOrDict<'a>),
    #[op_tag("BMC")]
    BeginMarkedContent(NameOfDict),
    #[op_tag("BDC")]
    BeginMarkedContentWithProperties(NameOfDict, NameOrDict<'a>),
    #[op_tag("EMC")]
    EndMarkedContent,

    // Compatibility Operations
    #[op_tag("BX")]
    BeginCompatibilitySection,
    #[op_tag("EX")]
    EndCompatibilitySection,
}

pub(crate) trait ConvertFromObject<'a, 'b>
where
    Self: Sized,
{
    fn convert_from_object(objects: &'b mut Vec<Object<'a>>) -> Result<Self, ObjectValueError>;
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
        let mut colors = ArrayVec::<f32, 4>::new();
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

impl<'a, 'b> ConvertFromObject<'a, 'b> for ColorOrName {
    fn convert_from_object(objects: &'b mut Vec<Object<'a>>) -> Result<Self, ObjectValueError> {
        let o = objects.pop().unwrap();
        if let Ok(name) = o.as_name() {
            Ok(ColorOrName::Name(name.to_owned()))
        } else {
            objects.push(o);
            Color::convert_from_object(objects).map(ColorOrName::Color)
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

impl<'a, 'b> ConvertFromObject<'a, 'b> for NameOrDict<'a> {
    fn convert_from_object(objects: &'b mut Vec<Object<'a>>) -> Result<Self, ObjectValueError> {
        match objects.pop().unwrap() {
            Object::Name(name) => Ok(NameOrDict::Name(name.to_owned())),
            Object::Dictionary(dict) => Ok(NameOrDict::Dict(dict)),
            _ => Err(ObjectValueError::GraphicsOperationSchemaError),
        }
    }
}

impl<'a, 'b> ConvertFromObject<'a, 'b> for Point {
    fn convert_from_object(objects: &'b mut Vec<Object<'a>>) -> Result<Self, ObjectValueError> {
        let y = objects.pop().unwrap().as_number()?;
        let x = objects.pop().unwrap().as_number()?;
        Ok(Self { x, y })
    }
}

#[derive(Debug, PartialEq)]
enum ObjectOrOperator<'a> {
    Object(Object<'a>),
    Operator(&'a str),
}

lazy_static! {
    static ref OPERATORS: HashSet<&'static str, RandomState> = [
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
    let p = is_not(b" \t\n\r%[<(".as_slice());
    map_res(p, |op| {
        let op = unsafe { std::str::from_utf8_unchecked(op) };
        if OPERATORS.contains(op) {
            Ok(ObjectOrOperator::Operator(op))
        } else {
            Err(ParseError::from_error_kind(input, ErrorKind::Tag))
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
                let r = (
                    input,
                    create_operation(op, &mut operands).map_err(|e| {
                        nom::Err::Error(ParseError::from_external_error(input, ErrorKind::Fail, e))
                    })?,
                );
                assert!(operands.is_empty());
                return Ok(r);
            }
        }
    }
}

pub fn parse_operations(input: &[u8]) -> ParseResult<Vec<Operation<'_>>> {
    ws_terminated(many0(parse_operation))(input)
}

#[cfg(test)]
mod tests;
