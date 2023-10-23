use std::{collections::HashSet, num::NonZeroU32};

use ahash::RandomState;
use anyhow::Result as AnyResult;

use educe::Educe;
use lazy_static::lazy_static;
use log::error;
use nom::{
    branch::alt,
    bytes::complete::is_not,
    combinator::map_res,
    error::{ErrorKind, FromExternalError, ParseError as NomParseError},
    multi::many0,
    Parser,
};
use tiny_skia::Transform;

use crate::{
    file::{Rectangle, ResourceDict},
    object::{
        Dictionary, Name, Object, ObjectValueError, PdfObject, Stream, TextString,
        TextStringOrNumber,
    },
    parser::{parse_object, ws_prefixed, ws_terminated, ParseError, ParseResult},
};
use nipdf_macro::{pdf_object, OperationParser, TryFromIntObject, TryFromNameObject};

pub(crate) mod color_space;
mod pattern;
use crate::file::ObjectResolver;
use crate::graphics::color_space::ColorSpace as ColorSpaceTrait;
pub(crate) use pattern::*;

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

impl From<TransformMatrix> for Transform {
    fn from(m: TransformMatrix) -> Self {
        Self::from_row(m.sx, m.ky, m.kx, m.sy, m.tx, m.ty)
    }
}

impl From<Transform> for TransformMatrix {
    fn from(m: Transform) -> Self {
        Self {
            sx: m.sx,
            kx: m.kx,
            ky: m.ky,
            sy: m.sy,
            tx: m.tx,
            ty: m.ty,
        }
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

impl<'a, 'b> ConvertFromObject<'a, 'b> for ColorArgs {
    fn convert_from_object(objects: &'b mut Vec<Object<'a>>) -> Result<Self, ObjectValueError> {
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

/// Predefined simple color space
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PredefinedColorSpace {
    DeviceGray,
    DeviceRGB,
    DeviceCMYK,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ColorSpaceArgs {
    Predefined(PredefinedColorSpace),

    CalGray,

    Pattern,
    ICCBased(NonZeroU32), // stream id
    /// Separation color space, the first element is the alternate color space,
    /// the second element is ref id of the tint transform function.
    Separation((Box<Self>, NonZeroU32)),

    /// Reference id to the ColorSpace object, need later resolve
    LaterResolve(NonZeroU32),

    /// User defined custom ColorSpace, resolve it from Resource Dictionary
    Custom(String),
}

#[pdf_object(())]
trait ICCStreamDictTrait {
    #[try_from]
    fn alternate(&self) -> Option<ColorSpaceArgs>;
}

impl ColorSpaceArgs {
    /// Convert args to ColorSpace, resolve from page resources if it is Custom.
    /// Panic if self is ColorSpaceArgs::Custom(), and resources is None,
    /// because in this case color space is defined in page resources.
    pub fn create_color_space<'a>(
        &self,
        resolver: &ObjectResolver<'a>,
        resources: Option<&ResourceDict<'a, '_>>,
    ) -> AnyResult<Box<dyn ColorSpaceTrait<f32>>> {
        match self {
            Self::Predefined(cs) => Ok(match cs {
                PredefinedColorSpace::DeviceGray => Box::new(color_space::DeviceGray()),
                PredefinedColorSpace::DeviceRGB => Box::new(color_space::DeviceRGB()),
                PredefinedColorSpace::DeviceCMYK => Box::new(color_space::DeviceCMYK()),
            }),
            Self::LaterResolve(id) => {
                let cs_owner: Self = resolver.resolve(*id)?.try_into()?;
                match &cs_owner {
                    ColorSpaceArgs::ICCBased(id) => {
                        let d = resolver.resolve(*id)?.as_stream()?.as_dict();
                        let d = ICCStreamDict::new(None, d, resolver)?;
                        let space_args = d
                            .alternate()?
                            .expect("unsupported if ICCBased color no alternate color space");
                        space_args.create_color_space(resolver, resources)
                    }
                    _ => todo!("Unsupported color space: {:?}", &cs_owner),
                }
            }
            Self::Custom(name) => {
                let spaces = resources
                    .expect("Page resources needed to handle Custom ColorSpaceArgs")
                    .color_space()?;
                let args = spaces.get(name).ok_or(ObjectValueError::DictNameMissing)?;
                args.create_color_space(resolver, resources)
            }
            _ => todo!("Convert {:?} to Color space", self),
        }
    }
}

impl<'a, 'b> TryFrom<&'b Object<'a>> for ColorSpaceArgs {
    type Error = ObjectValueError;
    fn try_from(object: &'b Object<'a>) -> Result<Self, Self::Error> {
        match object {
            Object::Name(name) => match name.as_ref() {
                "DeviceGray" => Ok(Self::Predefined(PredefinedColorSpace::DeviceGray)),
                "DeviceRGB" => Ok(Self::Predefined(PredefinedColorSpace::DeviceRGB)),
                "DeviceCMYK" => Ok(Self::Predefined(PredefinedColorSpace::DeviceCMYK)),
                "CalGray" => Ok(Self::CalGray),
                "Pattern" => Ok(Self::Pattern),
                _ => Ok(Self::Custom(name.as_ref().to_owned())),
            },
            Object::Array(arr) => match arr[0].as_name()? {
                "ICCBased" => {
                    assert_eq!(2, arr.len());
                    Ok(Self::ICCBased(arr[1].as_ref()?.id().id()))
                }
                "Separation" => {
                    assert_eq!(4, arr.len());
                    let cs = Box::new(Self::try_from(&arr[2])?);
                    let tint_transform = arr[3].as_ref()?.id().id();
                    Ok(Self::Separation((cs, tint_transform)))
                }
                _ => todo!("Unsupported color space: {:?}", arr),
            },
            Object::Reference(id) => Ok(Self::LaterResolve(id.id().id())),
            _ => {
                error!("{:?}", object);
                Err(ObjectValueError::GraphicsOperationSchemaError)
            }
        }
    }
}

impl<'a, 'b> ConvertFromObject<'a, 'b> for ColorSpaceArgs {
    fn convert_from_object(objects: &'b mut Vec<Object<'a>>) -> Result<Self, ObjectValueError> {
        let o = objects.pop().unwrap();
        ColorSpaceArgs::try_from(&o).map_err(|_| ObjectValueError::GraphicsOperationSchemaError)
    }
}

impl<'a, 'b, const N: usize> ConvertFromObject<'a, 'b> for [f32; N] {
    fn convert_from_object(objects: &'b mut Vec<Object<'a>>) -> Result<Self, ObjectValueError> {
        let mut result = [0.0; N];
        for i in 0..N {
            result[N - 1 - i] = objects.pop().unwrap().as_number()?;
        }
        Ok(result)
    }
}

pub fn cmyk_to_rgb8(c: f32, y: f32, m: f32, k: f32) -> (u8, u8, u8) {
    (
        ((1.0 - c) * (1.0 - k) * 255.0) as u8,
        ((1.0 - m) * (1.0 - k) * 255.0) as u8,
        ((1.0 - y) * (1.0 - k) * 255.0) as u8,
    )
}

impl<'a> TryFrom<&Object<'a>> for ColorArgs {
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
pub enum ColorArgsOrName {
    Color(ColorArgs),
    Name(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct NameOfDict(pub String);

#[derive(Debug, Clone, PartialEq)]
pub enum NameOrDict<'a> {
    Name(Name<'a>),
    Dict(Dictionary<'a>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum NameOrDictByRef<'a, 'b> {
    Name(&'b Name<'a>),
    Dict(&'b Dictionary<'a>),
}

impl<'a, 'b> TryFrom<&'b Object<'a>> for NameOrDictByRef<'a, 'b> {
    type Error = ObjectValueError;

    fn try_from(obj: &'b Object<'a>) -> Result<Self, Self::Error> {
        match obj {
            Object::Name(name) => Ok(NameOrDictByRef::Name(name)),
            Object::Dictionary(dict) => Ok(NameOrDictByRef::Dict(dict)),
            _ => Err(ObjectValueError::GraphicsOperationSchemaError),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum NameOrStream<'a, 'b> {
    Name(&'b Name<'a>),
    Stream(&'b Stream<'a>),
}

impl<'a, 'b> TryFrom<&'b Object<'a>> for NameOrStream<'a, 'b> {
    type Error = ObjectValueError;

    fn try_from(obj: &'b Object<'a>) -> Result<Self, Self::Error> {
        match obj {
            Object::Name(name) => Ok(NameOrStream::Name(name)),
            Object::Stream(stream) => Ok(NameOrStream::Stream(stream)),
            _ => Err(ObjectValueError::GraphicsOperationSchemaError),
        }
    }
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
    SetTextRenderingMode(TextRenderingMode),
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
    ShowText(TextString<'a>),
    #[op_tag("TJ")]
    ShowTexts(Vec<TextStringOrNumber<'a>>),
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

impl<'a, 'b, T: for<'c> ConvertFromObject<'a, 'c>> ConvertFromObject<'a, 'b> for Vec<T> {
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

impl<'a, 'b> ConvertFromObject<'a, 'b> for TextString<'a> {
    fn convert_from_object(objects: &'b mut Vec<Object<'a>>) -> Result<Self, ObjectValueError> {
        let o = objects.pop().unwrap();
        match o {
            Object::LiteralString(s) => Ok(TextString::Text(s)),
            Object::HexString(s) => Ok(TextString::HexText(s)),
            _ => Err(ObjectValueError::UnexpectedType),
        }
    }
}

impl<'a, 'b> ConvertFromObject<'a, 'b> for TextStringOrNumber<'a> {
    fn convert_from_object(objects: &'b mut Vec<Object<'a>>) -> Result<Self, ObjectValueError> {
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

impl<'a, 'b> ConvertFromObject<'a, 'b> for ColorArgsOrName {
    fn convert_from_object(objects: &'b mut Vec<Object<'a>>) -> Result<Self, ObjectValueError> {
        let o = objects.pop().unwrap();
        if let Ok(name) = o.as_name() {
            Ok(ColorArgsOrName::Name(name.to_owned()))
        } else {
            objects.push(o);
            ColorArgs::convert_from_object(objects).map(ColorArgsOrName::Color)
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
    let p = is_not(b" \t\n\r%[<(/".as_slice());
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
