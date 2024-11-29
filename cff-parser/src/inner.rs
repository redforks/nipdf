use paste::paste;
use prescript::{Encoding, Name, ParserError, name, sname};
use snafu::prelude::*;
use std::{
    borrow::Cow,
    collections::HashMap,
    hash::Hash,
    num::TryFromIntError,
    ops::{Deref, Range, RangeInclusive},
};
use winnow::{
    PResult, Parser,
    binary::{be_u8, be_u16, be_u24, be_u32, length_repeat, length_take},
    combinator::{alt, dispatch, empty, fail, preceded, repeat, repeat_till, rest, terminated},
    error::{AddContext, ErrorConvert, ErrorKind, FromExternalError, ParseError},
    stream::{Accumulate, Stream, StreamIsPartial},
    token::{any, take},
};

mod predefined_charsets;
mod predefined_encodings;

/// Glyph ID
type Gid = u8;

type Sid = u16;

/// Operand, value of Dict
#[derive(Clone, PartialEq, Debug)]
enum Operand {
    Integer(i32),
    Real(f32),
    IntArray(Vec<i32>),
    RealArray(Vec<f32>),
}

impl Operand {
    /// Return integer value if Operand is Integer, otherwise return None.
    pub fn int(&self) -> Option<i32> {
        match self {
            Operand::Integer(v) => Some(*v),
            _ => None,
        }
    }

    /// Return real value if Operand is Integer or Real, otherwise return None.
    pub fn real(&self) -> Option<f32> {
        match self {
            Operand::Integer(v) => Some(*v as f32),
            Operand::Real(v) => Some(*v),
            _ => None,
        }
    }

    /// Return bool value if Operand is Integer, otherwise return None.
    /// int 1 is true, 0 is false. Other int value is invalid.
    pub fn bool(&self) -> Option<bool> {
        match self {
            Operand::Integer(0) => Some(false),
            Operand::Integer(1) => Some(true),
            _ => None,
        }
    }

    pub fn int_array(&self) -> Option<&[i32]> {
        match self {
            Operand::IntArray(v) => Some(v),
            _ => None,
        }
    }

    pub fn real_array(&self) -> Option<&[f32]> {
        match self {
            Operand::RealArray(v) => Some(v),
            _ => None,
        }
    }
}

/// Return parser to parse integer
fn integer_parser<'a>() -> impl Parser<&'a [u8], i32, ParserError> {
    dispatch! {any;
        v@32..=246  => |_: &mut &[u8]| Ok((v as i32) - 139),
        v@247..=250 => |buf: &mut &[u8]| {
            let b1 = any(buf)?;
            Ok(((v as i32) - 247) * 256 + (b1 as i32) + 108)
        },
        v@251..=254 => |buf: &mut &[u8]| {
            let b1 = any(buf)?;
            Ok(-((v as i32) - 251) * 256 - (b1 as i32) - 108)
        },
        28 => |buf: &mut &[u8]| {
            let b1 = any(buf)?;
            let b2 = any(buf)?;
            Ok(((b1 as i16) << 8 | b2 as i16) as i32)
        },
        29 => |buf: &mut &[u8]| {
            let b1 = any(buf)?;
            let b2 = any(buf)?;
            let b3 = any(buf)?;
            let b4 = any(buf)?;
            Ok(((b1 as i32) << 24) + ((b2 as i32) << 16) + ((b3 as i32) << 8) + (b4 as i32))
        },
        _ => fail::<_, i32, _>,
    }
}

/// A real number operand is provided in addition to integer operands. This
/// operand begins with a byte value of 30 followed  by a variable-length
/// sequence of bytes. Each byte is composed  of two 4-bit nibbles as defined in
/// fowling table. The first nibble of a  pair is stored in the most significant 4
/// bits of a byte and the  second nibble of a pair is stored in the least
/// significant 4 bits of a byte.
///
/// | nibble | represents |
/// |--------|-------|
/// | 0-9 | 0-9 |
/// | a | .(decimal point) |
/// | b | E |
/// | c | E– |
/// | d | <reserved> |
/// | e | –(minus) |
/// | f | end of number |
///
/// A real number is terminated by one (or two) 0xf nibbles so that it is
/// always padded to a full byte. Thus, the value –2.25 is  encoded by the byte
/// sequence (1e e2 a2 5f) and the value  0.140541E–3 by the sequence (1e 0a 14
/// 05 41 c3 ff).
fn real_parser<'a>() -> impl Parser<&'a [u8], f32, ParserError> {
    use winnow::binary::bits::{bits, pattern, take};

    #[derive(PartialEq, Debug)]
    enum NumberState {
        Int,
        Mantissa,
        Exponent,
    }

    struct Real {
        int: u32,
        negative: bool,
        state: NumberState,
        mantissa: f32,
        mantissa_len: i32,
        exponent_negative: bool,
        exponent: u32,
    }

    impl From<Real> for f32 {
        fn from(value: Real) -> Self {
            let mut r = value.mantissa.mul_add(
                10f32.powi(-value.mantissa_len)
                    * 10f32.powf(if value.exponent_negative {
                        -(value.exponent as f32)
                    } else {
                        value.exponent as f32
                    }),
                value.int as f32,
            );
            if value.negative {
                r = -r;
            }
            r
        }
    }

    impl Accumulate<u8> for Real {
        fn initial(_: Option<usize>) -> Self {
            Self {
                int: 0,
                state: NumberState::Int,
                exponent_negative: false,
                mantissa: 0.0,
                mantissa_len: 0,
                exponent: 0,
                negative: false,
            }
        }

        fn accumulate(&mut self, acc: u8) {
            match acc {
                0..=9 => match self.state {
                    NumberState::Int => {
                        self.int = self.int * 10 + acc as u32;
                    }
                    NumberState::Mantissa => {
                        self.mantissa = self.mantissa.mul_add(10.0, acc as f32);
                        self.mantissa_len += 1;
                    }
                    NumberState::Exponent => {
                        self.exponent = self.exponent * 10 + acc as u32;
                    }
                },
                0xa => {
                    debug_assert_eq!(NumberState::Int, self.state);
                    self.state = NumberState::Mantissa;
                }
                0xb => {
                    self.state = NumberState::Exponent;
                }
                0xc => {
                    self.state = NumberState::Exponent;
                    self.exponent_negative = true;
                }
                0xe => {
                    // minus
                    self.negative = true;
                }
                _ => unreachable!(),
            }
        }
    }
    preceded(
        30u8,
        bits(repeat_till::<_, _, Real, _, _, _, _>(
            1..,
            take::<_, u8, _, ParserError>(4u8),
            pattern(0xfu8, 4u8),
        )),
    )
    .map(|(v, _)| v.into())
}

/// Return operand parser
/// Operand maybe integer/real/bool/intArray/realArray, if multiple operands
/// are provided, item types must be same, either int or real, returned as
/// intArray/realArray.
fn operand_parser<'a>() -> impl Parser<&'a [u8], Operand, ParserError> {
    fn post_process(v: Vec<Operand>) -> Operand {
        // if v has one element, return that element
        // if all elements are all int, return int_array
        // if all elements are all real, return real_array
        // otherwize, convert all elements to real, and return real_array
        if v.len() == 1 {
            return v[0].clone();
        }
        let mut is_same_type = true;
        let mut is_int = false;
        let mut is_real = false;
        for i in &v {
            match i {
                Operand::Integer(_) => {
                    if is_real {
                        is_same_type = false;
                        break;
                    }
                    is_int = true;
                }
                Operand::Real(_) => {
                    if is_int {
                        is_same_type = false;
                        break;
                    }
                    is_real = true;
                }
                _ => {
                    is_same_type = false;
                    break;
                }
            }
        }
        if is_same_type {
            if is_int {
                let mut int_array = Vec::with_capacity(v.len());
                for i in v {
                    match i {
                        Operand::Integer(i) => int_array.push(i),
                        _ => unreachable!(),
                    }
                }
                Operand::IntArray(int_array)
            } else if is_real {
                let mut real_array = Vec::with_capacity(v.len());
                for i in v {
                    match i {
                        Operand::Real(r) => real_array.push(r),
                        _ => unreachable!(),
                    }
                }
                Operand::RealArray(real_array)
            } else {
                unreachable!()
            }
        } else {
            // mixed int/real to real array
            let mut real_array = Vec::with_capacity(v.len());
            for i in v {
                match i {
                    Operand::Integer(i) => real_array.push(i as f32),
                    Operand::Real(r) => real_array.push(r),
                    _ => unreachable!(),
                }
            }
            Operand::RealArray(real_array)
        }
    }

    repeat(
        1..,
        alt((
            integer_parser().map(Operand::Integer),
            real_parser().map(Operand::Real),
        )),
    )
    .map(post_process)
}

/// Operator of Dict. Operator is a byte value that is either a single byte
/// value 0-21 or a byte value equal to 12 followed by a single byte
/// value 0-21.
#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub struct Operator {
    tag: u8,
    /// First byte of Operator is 12 if true.
    escape: bool,
}

impl Operator {
    pub const BASE_FONT_BLEND: Self = Self::escaped(23);
    pub const BASE_FONT_NAME: Self = Self::escaped(22);
    pub const CHARSETS: Self = Self::new(15);
    pub const CHARSTRING_TYPE: Self = Self::escaped(6);
    pub const CHAR_STRINGS: Self = Self::new(17);
    pub const COPYRIGHT: Self = Self::escaped(0);
    pub const ENCODINGS: Self = Self::new(16);
    pub const FAMILY_NAME: Self = Self::new(3);
    pub const FONT_BBOX: Self = Self::new(5);
    pub const FONT_MATRIX: Self = Self::escaped(7);
    pub const FULL_NAME: Self = Self::new(2);
    pub const IS_FIXED_PITCH: Self = Self::escaped(1);
    pub const ITALIC_ANGLE: Self = Self::escaped(2);
    pub const NOTICE: Self = Self::new(1);
    pub const PAINT_TYPE: Self = Self::escaped(5);
    pub const POST_SCRIPT: Self = Self::escaped(21);
    pub const PRIVATE: Self = Self::new(18);
    pub const ROS: Self = Self::escaped(30);
    pub const STROKE_WIDTH: Self = Self::escaped(8);
    pub const SYNTHETIC_BASE: Self = Self::escaped(20);
    pub const UNDERLINE_POSITION: Self = Self::escaped(3);
    pub const UNDERLINE_THICKNESS: Self = Self::escaped(4);
    pub const UNIQUE_ID: Self = Self::new(13);
    pub const VERSION: Self = Self::new(0);
    pub const WEIGHT: Self = Self::new(4);
    pub const XUID: Self = Self::new(14);

    pub const fn new(tag: u8) -> Self {
        debug_assert!(tag <= 21);
        Self { tag, escape: false }
    }

    pub const fn escaped(tag: u8) -> Self {
        Self { tag, escape: true }
    }
}

/// Operator hash is tag, if escape is true, set high bit.
impl Hash for Operator {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let tag = self.tag;
        let escape = self.escape;
        let tag = if escape { tag | 0x80 } else { tag };
        tag.hash(state);
    }
}

fn operator_parser<'a>() -> impl Parser<&'a [u8], Operator, ParserError> {
    let escaped = preceded(12u8, any).map(Operator::escaped);
    let normal = any.map(Operator::new);
    alt((escaped, normal))
}

/// Error may returned in this crate.
#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Dict value not Integer"))]
    ExpectInt,
    #[snafu(display("Dict value not Real"))]
    ExpectReal,
    #[snafu(display("Dict value not Integer Array"))]
    ExpectIntArray,
    #[snafu(display("Dict value not Real Array"))]
    ExpectRealArray,
    #[snafu(display("Dict value not Bool"))]
    ExpectBool,

    #[snafu(display("Invalid offsets data"))]
    InvalidOffsetsData,

    #[snafu(display("Parse error: {message}"))]
    ParseError {
        message: String,
        source: ParserError,
    },

    /// Error during cast integer.
    #[snafu(display("Parse error: {message}"))]
    ParseErrorIntCast {
        message: String,
        source: TryFromIntError,
    },

    #[snafu(display("Required top dict value missing"))]
    RequiredDictValueMissing,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(PartialEq, Debug, Clone)]
pub struct Dict(HashMap<Operator, Operand>);

impl Dict {
    /// If value not exist for `k`, return None,
    /// use `f` to convert Operand value to type `T` and returns otherwise.
    fn opt<'a, T: 'a, F: FnOnce(&'a Operand) -> Result<T>>(
        &'a self,
        f: F,
        k: Operator,
    ) -> Result<Option<T>> {
        self.0.get(&k).map(f).transpose()
    }

    /// If value not exist for `k`, return default value `dv`,
    /// use `f` to convert Operand value to type `T` and returns otherwise.
    fn opt_or<'a, T: 'a, F: FnOnce(&'a Operand) -> Result<T>>(
        &'a self,
        f: F,
        k: Operator,
        dv: T,
    ) -> Result<T> {
        self.0.get(&k).map_or(Ok(dv), f)
    }

    /// If value not exist for `k`, return `Error::RequiredDictValueMissing` error,
    /// use `f` to convert Operand value to type `T` and returns otherwise.
    fn required<'a, T: 'a, F: FnOnce(&'a Operand) -> Result<T>>(
        &'a self,
        f: F,
        k: Operator,
    ) -> Result<T> {
        self.0
            .get(&k)
            .map_or(Err(Error::RequiredDictValueMissing), f)
    }

    /// Assume the operand value is delta-encoded, return decoded real number array.
    pub fn as_delta_encoded(&self, k: Operator) -> Result<Option<Vec<f32>>> {
        let r = self.as_real_array(k)?;
        Ok(r.map(|v| {
            let mut r = Vec::with_capacity(v.len());
            let mut prev = 0.0;
            for &i in v {
                r.push(i + prev);
                prev += i;
            }
            r
        }))
    }

    /// Assume the operand value is delta-encoded, return decoded real number array.
    /// Return default value if value not exist.
    pub fn as_delta_encoded_or(&self, k: Operator, default: &'static [f32]) -> Result<Vec<f32>> {
        self.as_delta_encoded(k)
            .map(|v| v.unwrap_or_else(|| default.to_vec()))
    }

    pub fn delta_encoded(&self, k: Operator) -> Result<Vec<f32>> {
        self.as_delta_encoded(k)
            .and_then(|v| v.ok_or(Error::RequiredDictValueMissing))
    }
}

impl Accumulate<(Operand, Operator)> for Dict {
    fn initial(capacity: Option<usize>) -> Self {
        Dict(capacity.map_or_else(HashMap::new, HashMap::with_capacity))
    }

    fn accumulate(&mut self, acc: (Operand, Operator)) {
        self.0.insert(acc.1, acc.0);
    }
}

macro_rules! access_methods {
    ($name: ident, $f: expr, $rt: ty) => {
        access_methods!($name, $f, $rt, $rt);
    };
    ($name: ident, $f: expr, $rt: ty, $def_t: ty) => {
        paste! {
            pub fn $name(&self, k: Operator) -> Result<$rt> {
                self.required($f, k)
            }

            pub fn [<as_ $name>](&self, k: Operator) -> Result<Option<$rt>> {
                self.opt($f, k)
            }

            pub fn [<as_ $name _or>](&self, k: Operator, default: $def_t) -> Result<$rt> {
                self.opt_or($f, k, default)
            }
        }
    };
}

impl Dict {
    access_methods!(int, |v| v.int().context(ExpectIntSnafu), i32);

    access_methods!(real, |v| v.real().context(ExpectRealSnafu), f32);

    access_methods!(bool, |v| v.bool().context(ExpectBoolSnafu), bool);

    access_methods!(
        int_array,
        |v| v.int_array().context(ExpectIntArraySnafu),
        &[i32],
        &'static [i32]
    );

    access_methods!(
        real_array,
        |v| v.real_array().context(ExpectRealArraySnafu),
        &[f32],
        &'static [f32]
    );
}

/// Return Dict parser.
/// Dict stored as a sequence of operators and operands. The operands are
/// stored before the operators.
fn dict_parser<'a>() -> impl Parser<&'a [u8], Dict, ParserError> {
    let parse_item = (operand_parser(), operator_parser());
    repeat(1.., parse_item)
}

/// Byte length of offset data type.
#[derive(PartialEq, Eq, Debug, Clone, Copy)]
#[repr(u8)]
pub enum OffSize {
    One = 1u8,
    Two = 2u8,
    Three = 3u8,
    Four = 4u8,
}

impl OffSize {
    /// Return byte length of offset data type.
    pub fn len(self) -> usize {
        self as usize
    }
}

fn off_size_parser<'a>() -> impl Parser<&'a [u8], OffSize, ParserError> {
    dispatch! {any;
        1 => empty.value(OffSize::One),
        2 => empty.value(OffSize::Two),
        3 => empty.value(OffSize::Three),
        4 => empty.value(OffSize::Four),
        _ => fail
    }
}

/// Offsets is a sequence of n + 1 off_size bytes, where n is the number of
/// items in the index. The first offset is always 1.
#[derive(Debug, Clone, Copy)]
struct Offsets<'a>(OffSize, &'a [u8]);

impl<'a> Offsets<'a> {
    /// Return `Error::InvalidOffsetsData` if first offset is not 1.
    /// Assume data byte length is multiple of off_size.
    pub fn new(off_size: OffSize, data: &'a [u8]) -> Result<Self> {
        let first = Self::_get(data, off_size, 0)?;
        ensure!(first == 1, InvalidOffsetsDataSnafu);
        Ok(Self(off_size, data))
    }

    /// Return length of offsets, which is the number of elements.
    pub fn len(&self) -> usize {
        self.1.len() / self.0.len() - 1
    }

    /// Return data offset range of specific index. Panic if `ith` is out of range.
    pub fn range(&self, ith: usize) -> Range<usize> {
        self.get(ith)..self.get(ith + 1)
    }

    /// Return data offset of specific index. The offset is 0-based.
    /// `ith` can be length of offsets, which means the end offset of last element.
    /// Panic if `ith` is out of range.
    pub fn get(&self, ith: usize) -> usize {
        let r = Self::_get(self.1, self.0, ith)
            .unwrap_or_else(|e| panic!("parse offset failed: {:?}", e));
        r as usize - 1
    }

    /// Get offset of `ith` element
    fn _get(data: &[u8], off_size: OffSize, ith: usize) -> Result<u32> {
        // skip ith off_size bytes
        let buf = &data[ith * off_size.len()..];
        match off_size {
            OffSize::One => ignore_rest(be_u8.map(|v| v as u32)).parse(buf),
            OffSize::Two => ignore_rest(be_u16.map(|v| v as u32)).parse(buf),
            OffSize::Three => ignore_rest(be_u24.map(|v| v)).parse(buf),
            OffSize::Four => ignore_rest(be_u32).parse(buf),
        }
        .map_err(Into::<ParserError>::into)
        .context(ParseSnafu {
            message: "parse OffSize".to_owned(),
        })
    }
}

fn ignore_rest<I, O, E, P>(p: P) -> impl Parser<I, O, E>
where
    I: Stream,
    E: winnow::error::ParserError<I>,
    P: Parser<I, O, E>,
{
    terminated(p, rest)
}

fn parse_ignore_rest<I, O, P>(p: P, buf: I) -> Result<O, ParserError>
where
    I: Stream + StreamIsPartial,
    P: Parser<I, O, ParserError>,
{
    ignore_rest(p).parse(buf).map_err(Into::into)
}

/// Data with an index(offset) for quick access memory
/// by index.
#[derive(Debug, Clone, Copy)]
pub struct IndexedData<'a> {
    offsets: Offsets<'a>,
    data: &'a [u8],
}

impl<'a> IndexedData<'a> {
    pub fn len(&self) -> usize {
        self.offsets.len()
    }

    /// Get value by index, use parser to decode data.
    /// Panic if `idx` is out of range.
    pub fn get<T: 'a, F: Parser<&'a [u8], T, ParserError>>(
        &self,
        idx: usize,
        mut f: F,
    ) -> Result<T> {
        let buf = self.get_bin_str(idx);
        f.parse(buf).map_err(Into::into).context(ParseSnafu {
            message: format!("get indexed data: [{}]", idx),
        })
    }

    /// Get str by index. Panic if `idx` is out of range.
    /// Returns `&[u8]` instead of `&str`, because the str may not be valid utf8,
    /// `from_utf8()` returns error if str contains '\0'.
    pub fn get_bin_str(&self, idx: usize) -> &'a [u8] {
        let range = self.offsets.range(idx);
        &self.data[range]
    }

    /// Get Dict by index. Panic if `idx` is out of range.
    pub fn get_dict(&self, idx: usize) -> Result<Dict> {
        self.get(idx, dict_parser())
    }
}

/// Index Format:
///
/// ---+-----------------------+------------------------------------------
/// 0 | count  | The number of index entries
/// ---+-----------------------+------------------------------------------
/// 1 | off_size              | The size in bytes of each offset
/// ---+-----------------------+------------------------------------------
/// 2 | offset array          | Offset array, count + 1 elements
/// ---+-----------------------+------------------------------------------
/// 3 | data                  | Data
/// ---+-----------------------+------------------------------------------
fn parse_indexed_data<'a>(buf: &'_ mut &'a [u8]) -> PResult<IndexedData<'a>, ParserError> {
    let (n, off_size) = (be_u16, off_size_parser()).parse_next(buf)?;
    let offset_data_len = (n + 1) as usize * off_size.len();
    let offsets = take(offset_data_len)
        .try_map(|offset_data| Offsets::new(off_size, offset_data))
        .parse_next(buf)?;

    let data_len = offsets.get(n as usize);
    take(data_len)
        .map(|data| IndexedData { offsets, data })
        .parse_next(buf)
}

fn name_index_parser<'a>() -> impl Parser<&'a [u8], NameIndex<'a>, ParserError> {
    parse_indexed_data.map(NameIndex)
}

fn string_index_parser<'a>() -> impl Parser<&'a [u8], StringIndex<'a>, ParserError> {
    parse_indexed_data.map(StringIndex)
}

fn top_dict_index_parser<'a>() -> impl Parser<&'a [u8], TopDictIndex<'a>, ParserError> {
    parse_indexed_data.map(TopDictIndex)
}

/// Header of CFF.
#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub struct Header {
    pub major: u8,
    pub minor: u8,
    pub hdr_size: u8,
    pub off_size: OffSize,
}

fn header_parser<'a>() -> impl Parser<&'a [u8], Header, ParserError> {
    (be_u8, be_u8, be_u8, off_size_parser()).map(|(major, minor, hdr_size, off_size)| Header {
        major,
        minor,
        hdr_size,
        off_size,
    })
}

pub fn parse_header(buf: &[u8]) -> Result<Header> {
    parse_ignore_rest(header_parser(), buf).context(ParseSnafu {
        message: "parse header".to_owned(),
    })
}

/// Font name index, stores font names in Index.
/// The name first byte maybe zero, which means the corresponding font
/// is removed. The index is the index of other top font data index.
#[derive(Debug, Clone, Copy)]
pub struct NameIndex<'a>(IndexedData<'a>);

impl<'a> NameIndex<'a> {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Get font name by index. Return None if name is marked removed.
    pub fn get(&self, idx: usize) -> Option<Cow<'a, str>> {
        let name = self.0.get_bin_str(idx);
        if name.is_empty() || name[0] == 0 {
            None
        } else {
            Some(String::from_utf8_lossy(name))
        }
    }
}

/// Resolve &str using SID from IndexedData.
/// SID is an integer that identifies a string in the string INDEX.
/// The first 391 SIDs are predefined standard strings.
/// SID greater than 390 are strings that are defined in the string INDEX.
/// To resolve a SID, subtract 391 from the SID value and use the result as
/// an index into the string INDEX.
#[derive(Debug, Copy, Clone)]
pub struct StringIndex<'a>(IndexedData<'a>);

impl<'a> StringIndex<'a> {
    /// Panic if `idx` is out of range. Return None if str is marked removed
    pub fn get(&self, idx: Sid) -> Cow<'a, str> {
        if idx < 391 {
            Cow::Borrowed(STANDARD_STRINGS[idx as usize])
        } else {
            String::from_utf8_lossy(self.0.get_bin_str((idx - 391) as usize))
        }
    }
}

/// Standard strings defined in CFF spec, used in Type 1 and some other strings.
#[rustfmt::skip]
const STANDARD_STRINGS: [&str; 391] = [
    ".notdef", "space", "exclam", "quotedbl", "numbersign", "dollar", "percent",
    "ampersand", "quoteright", "parenleft", "parenright", "asterisk", "plus", "comma",
    "hyphen", "period", "slash", "zero", "one", "two", "three", "four", "five", "six",
    "seven", "eight", "nine", "colon", "semicolon", "less", "equal", "greater",
    "question", "at",
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P",
    "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
    "bracketleft", "backslash", "bracketright", "asciicircum", "underscore", "quoteleft",
    "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p",
    "q", "r", "s", "t", "u", "v", "w", "x", "y", "z",
    "braceleft", "bar", "braceright", "asciitilde", "exclamdown", "cent", "sterling",
    "fraction", "yen", "florin", "section", "currency", "quotesingle", "quotedblleft",
    "guillemotleft", "guilsinglleft", "guilsinglright", "fi", "fl", "endash", "dagger",
    "daggerdbl", "periodcentered", "paragraph", "bullet", "quotesinglbase",
    "quotedblbase", "quotedblright", "guillemotright", "ellipsis", "perthousand",
    "questiondown", "grave", "acute", "circumflex", "tilde", "macron", "breve",
    "dotaccent", "dieresis", "ring", "cedilla", "hungarumlaut", "ogonek", "caron",
    "emdash", "AE", "ordfeminine", "Lslash", "Oslash", "OE", "ordmasculine", "ae",
    "dotlessi", "lslash", "oslash", "oe", "germandbls", "onesuperior", "logicalnot",
    "mu", "trademark", "Eth", "onehalf", "plusminus", "Thorn", "onequarter", "divide",
    "brokenbar", "degree", "thorn", "threequarters", "twosuperior", "registered",
    "minus", "eth", "multiply", "threesuperior", "copyright", "Aacute", "Acircumflex",
    "Adieresis", "Agrave", "Aring", "Atilde", "Ccedilla", "Eacute", "Ecircumflex",
    "Edieresis", "Egrave", "Iacute", "Icircumflex", "Idieresis", "Igrave", "Ntilde",
    "Oacute", "Ocircumflex", "Odieresis", "Ograve", "Otilde", "Scaron", "Uacute",
    "Ucircumflex", "Udieresis", "Ugrave", "Yacute", "Ydieresis", "Zcaron", "aacute",
    "acircumflex", "adieresis", "agrave", "aring", "atilde", "ccedilla", "eacute",
    "ecircumflex", "edieresis", "egrave", "iacute", "icircumflex", "idieresis",
    "igrave", "ntilde", "oacute", "ocircumflex", "odieresis", "ograve", "otilde",
    "scaron", "uacute", "ucircumflex", "udieresis", "ugrave", "yacute", "ydieresis",
    "zcaron", "exclamsmall", "Hungarumlautsmall", "dollaroldstyle", "dollarsuperior",
    "ampersandsmall", "Acutesmall", "parenleftsuperior", "parenrightsuperior",
    "twodotenleader", "onedotenleader", "zerooldstyle", "oneoldstyle", "twooldstyle",
    "threeoldstyle", "fouroldstyle", "fiveoldstyle", "sixoldstyle", "sevenoldstyle",
    "eightoldstyle", "nineoldstyle", "commasuperior", "threequartersemdash",
    "periodsuperior", "questionsmall", "asuperior", "bsuperior", "centsuperior",
    "dsuperior", "esuperior", "isuperior", "lsuperior", "msuperior", "nsuperior",
    "osuperior", "rsuperior", "ssuperior", "tsuperior", "ff", "ffi", "ffl",
    "parenleftinferior", "parenrightinferior", "Circumflexsmall", "hyphensuperior",
    "Gravesmall", "Asmall", "Bsmall", "Csmall", "Dsmall", "Esmall", "Fsmall",
    "Gsmall", "Hsmall", "Ismall", "Jsmall", "Ksmall", "Lsmall", "Msmall", "Nsmall",
    "Osmall", "Psmall", "Qsmall", "Rsmall", "Ssmall", "Tsmall", "Usmall", "Vsmall",
    "Wsmall", "Xsmall", "Ysmall", "Zsmall", "colonmonetary", "onefitted", "rupiah",
    "Tildesmall", "exclamdownsmall", "centoldstyle", "Lslashsmall", "Scaronsmall",
    "Zcaronsmall", "Dieresissmall", "Brevesmall", "Caronsmall", "Dotaccentsmall",
    "Macronsmall", "figuredash", "hypheninferior", "Ogoneksmall", "Ringsmall",
    "Cedillasmall", "questiondownsmall", "oneeighth", "threeeighths", "fiveeighths",
    "seveneighths", "onethird", "twothirds", "zerosuperior", "foursuperior",
    "fivesuperior", "sixsuperior", "sevensuperior", "eightsuperior", "ninesuperior",
    "zeroinferior", "oneinferior", "twoinferior", "threeinferior", "fourinferior",
    "fiveinferior", "sixinferior", "seveninferior", "eightinferior", "nineinferior",
    "centinferior", "dollarinferior", "periodinferior", "commainferior",
    "Agravesmall", "Aacutesmall", "Acircumflexsmall", "Atildesmall", "Adieresissmall",
    "Aringsmall", "AEsmall", "Ccedillasmall", "Egravesmall", "Eacutesmall",
    "Ecircumflexsmall", "Edieresissmall", "Igravesmall", "Iacutesmall",
    "Icircumflexsmall", "Idieresissmall", "Ethsmall", "Ntildesmall", "Ogravesmall",
    "Oacutesmall", "Ocircumflexsmall", "Otildesmall", "Odieresissmall", "OEsmall",
    "Oslashsmall", "Ugravesmall", "Uacutesmall", "Ucircumflexsmall", "Udieresissmall",
    "Yacutesmall", "Thornsmall", "Ydieresissmall", "001.000", "001.001", "001.002",
    "001.003", "Black", "Bold", "Book", "Light", "Medium", "Regular", "Roman",
    "Semibold",
];

/// Dict supports resolve SID to &str
#[derive(Debug)]
struct SIDDict<'a> {
    dict: Dict,
    strings: StringIndex<'a>,
}

/// SIDDict deref to Dict, to add Dict access methods.
impl Deref for SIDDict<'_> {
    type Target = Dict;

    fn deref(&self) -> &Self::Target {
        &self.dict
    }
}

impl SIDDict<'_> {
    fn resolve_sid(&self, v: &Operand) -> Result<Cow<'_, str>> {
        v.int().context(ExpectIntSnafu).and_then(|v| {
            Ok(self.strings.get(
                v.try_into()
                    .context(ParseErrorIntCastSnafu { message: "" })?,
            ))
        })
    }

    pub fn sid(&self, k: Operator) -> Result<Cow<'_, str>> {
        self.required(|v| self.resolve_sid(v), k)
    }

    #[allow(dead_code)]
    pub fn as_sid(&self, k: Operator) -> Result<Option<Cow<'_, str>>> {
        self.opt(|v| self.resolve_sid(v), k)
    }

    #[allow(dead_code)]
    pub fn as_sid_or(&self, k: Operator, default: &'static str) -> Result<Cow<'_, str>> {
        self.opt_or(|v| self.resolve_sid(v), k, default.into())
    }
}

/// Top Dict for each font face.
#[derive(Debug)]
pub struct TopDictData<'a>(SIDDict<'a>);

impl<'a> TopDictData<'a> {
    pub fn new(dict: Dict, strings: StringIndex<'a>) -> Self {
        let r = Self(SIDDict { dict, strings });
        assert!(!r.0.0.contains_key(&Operator::ROS), "TODO: CIDFont");
        r
    }

    pub fn string_index(&self) -> StringIndex<'_> {
        self.0.strings
    }

    pub fn version(&self) -> Result<Cow<'_, str>> {
        self.0.sid(Operator::VERSION)
    }

    pub fn notice(&self) -> Result<Cow<'_, str>> {
        self.0.sid(Operator::NOTICE)
    }

    pub fn copyright(&self) -> Result<Cow<'_, str>> {
        self.0.sid(Operator::COPYRIGHT)
    }

    pub fn full_name(&self) -> Result<Cow<'_, str>> {
        self.0.sid(Operator::FULL_NAME)
    }

    pub fn family_name(&self) -> Result<Cow<'_, str>> {
        self.0.sid(Operator::FAMILY_NAME)
    }

    pub fn weight(&self) -> Result<Cow<'_, str>> {
        self.0.sid(Operator::WEIGHT)
    }

    pub fn is_fixed_pitch(&self) -> Result<bool> {
        self.0.as_bool_or(Operator::IS_FIXED_PITCH, false)
    }

    pub fn italic_angle(&self) -> Result<f32> {
        self.0.as_real_or(Operator::ITALIC_ANGLE, 0.0)
    }

    pub fn underline_position(&self) -> Result<f32> {
        self.0.as_real_or(Operator::UNDERLINE_POSITION, -100.0)
    }

    pub fn underline_thickness(&self) -> Result<f32> {
        self.0.as_real_or(Operator::UNDERLINE_THICKNESS, 50.0)
    }

    pub fn paint_type(&self) -> Result<i32> {
        self.0.as_int_or(Operator::PAINT_TYPE, 0)
    }

    pub fn charstring_type(&self) -> Result<i32> {
        self.0.as_int_or(Operator::CHARSTRING_TYPE, 2)
    }

    pub fn font_matrix(&self) -> Result<&[f32]> {
        self.0.as_real_array_or(
            Operator::FONT_MATRIX,
            &[0.001, 0.0, 0.0, 0.001, 0.0, 0.0][..],
        )
    }

    pub fn unique_id(&self) -> Result<i32> {
        self.0.as_int_or(Operator::UNIQUE_ID, 0)
    }

    pub fn font_bbox(&self) -> Result<&[f32]> {
        self.0
            .as_real_array_or(Operator::FONT_BBOX, &[0.0, 0.0, 0.0, 0.0][..])
    }

    pub fn stroke_width(&self) -> Result<f32> {
        self.0.as_real_or(Operator::STROKE_WIDTH, 0.0)
    }

    pub fn xuid(&self) -> Result<&[i32]> {
        self.0.int_array(Operator::XUID)
    }

    /// `file` is the raw file data.
    pub fn charsets(&self, file: &[u8]) -> Result<Charsets> {
        let offset = self.0.as_int_or(Operator::CHARSETS, 0)?;

        match offset {
            0 => Ok(Charsets::Predefined(PredefinedCharsets::ISOAdobe)),
            1 => Ok(Charsets::Predefined(PredefinedCharsets::Expert)),
            2 => Ok(Charsets::Predefined(PredefinedCharsets::ExpertSubset)),
            _ => parse_ignore_rest(
                charsets_parser(self.n_glyphs(file)?),
                &file[offset as usize..],
            )
            .context(ParseSnafu {
                message: "parse Charsets".to_owned(),
            }),
        }
    }

    pub fn encodings(&self, file: &[u8]) -> Result<(Encodings, Option<Vec<EncodingSupplement>>)> {
        let offset = self.0.as_int_or(Operator::ENCODINGS, 0)?;

        match offset {
            0 => Ok((Encodings::PredefinedStandard, None)),
            1 => Ok((Encodings::PredefinedExpert, None)),
            _ => parse_ignore_rest(encodings_parser(), &file[offset as usize..]).context(
                ParseSnafu {
                    message: "parse Encodings".to_owned(),
                },
            ),
        }
    }

    pub fn private(&self) -> Result<&[i32]> {
        self.0.int_array(Operator::PRIVATE)
    }

    fn char_strings(&self) -> Result<i32> {
        self.0.int(Operator::CHAR_STRINGS)
    }

    /// Return glyphs count in font. `file` is the raw file data.
    pub fn n_glyphs(&self, file: &[u8]) -> Result<u16> {
        let buf = &file[self.char_strings()? as usize..];
        let index = parse_ignore_rest(parse_indexed_data, buf).context(ParseSnafu {
            message: "parse CharStrings INDEX".to_owned(),
        })?;
        index.len().try_into().context(ParseErrorIntCastSnafu {
            message: "convert index length to u16".to_owned(),
        })
    }

    pub fn synthetic_base(&self) -> Result<i32> {
        self.0.int(Operator::SYNTHETIC_BASE)
    }

    pub fn post_script(&self) -> Result<Cow<'_, str>> {
        self.0.sid(Operator::POST_SCRIPT)
    }

    pub fn base_font_name(&self) -> Result<Cow<'_, str>> {
        self.0.sid(Operator::BASE_FONT_NAME)
    }

    pub fn base_font_blend(&self) -> Result<Vec<f32>> {
        self.0.delta_encoded(Operator::BASE_FONT_BLEND)
    }
}

/// IndexedData to store TopDicts. Each item is TopDict
pub struct TopDictIndex<'a>(IndexedData<'a>);

impl<'a> TopDictIndex<'a> {
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn get(&self, idx: usize, strings: StringIndex<'a>) -> Result<TopDictData<'a>> {
        Ok(TopDictData::new(self.0.get_dict(idx)?, strings))
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PredefinedCharsets {
    ISOAdobe = 0,
    Expert = 1,
    ExpertSubset = 2,
}

/// Charsets map code index(gid) (u8) to SID
#[derive(Debug, PartialEq)]
pub enum Charsets {
    /// Format0, n_glyph - 1 codes stored in u8 array. because 0 are omitted because it
    /// is always map to sid 0, which is .notdef.
    Format0(Vec<Sid>),
    Format1(Vec<RangeInclusive<Sid>>), // (first, n_left: u8)
    Format2(Vec<RangeInclusive<Sid>>), // (first, n_left: u16)
    Predefined(PredefinedCharsets),
}

impl Charsets {
    /// Return SID by index(gid). Return None if `idx` is out of range.
    pub fn resolve_sid(&self, idx: Gid) -> Option<Sid> {
        if idx == 0 {
            return Some(0);
        }

        match self {
            Self::Predefined(predefined) => match predefined {
                PredefinedCharsets::ISOAdobe => (idx < 229).then_some(idx as Sid),
                PredefinedCharsets::Expert => {
                    predefined_charsets::EXPERT.get(idx as usize).copied()
                }
                PredefinedCharsets::ExpertSubset => predefined_charsets::EXPERT_SUBSET
                    .get(idx as usize)
                    .copied(),
            },

            // 0 not stored in sids vec.
            Self::Format0(sids) => sids.get(idx as usize - 1).copied(),

            Self::Format1(ranges) | Self::Format2(ranges) => {
                let idx = idx as Sid;
                let mut i: Sid = 1;
                for range in ranges {
                    let start = i;
                    match Sid::try_from(range.len()) {
                        Ok(len) => i += len,
                        Err(e) => {
                            log::error!("Error converting range length to Sid: {:?}", e);
                            #[cfg(debug_assertions)]
                            panic!("Error converting range length to Sid: {:?}", e);
                            #[cfg(not(debug_assertions))]
                            {
                                log::error!("Error converting range length to Sid: {:?}", e);
                                return None;
                            }
                        }
                    }
                    if i > idx {
                        return Some(*range.start() + idx - start);
                    }
                }
                None
            }
        }
    }
}

/// Charsets has four formats by first byte of buf:
///
/// 0: format0, n_glyphs SID
/// 1: format1, n_ranges (first, n_left: u8) SID
/// 2: format2, n_ranges (first, n_left: u16) SID
///
/// Predefined charsets has no format byte, handled by TopDict::charsets().
fn charsets_parser<'a>(n_glyphs: u16) -> impl Parser<&'a [u8], Charsets, ParserError> {
    fn covers(r: &[RangeInclusive<Sid>]) -> usize {
        let mut covers = 0;
        for range in r {
            covers += range.len();
        }
        covers
    }

    fn range_parser<'a, LEFT: Parser<&'a [u8], u16, ParserError>>(
        n_glyphs: u16,
        mut n_left_parser: LEFT,
    ) -> impl Parser<&'a [u8], Vec<RangeInclusive<Sid>>, ParserError> {
        // let n_left_parser = n_left_parser();
        move |buf: &mut &'a [u8]| {
            let mut parse_item =
                (be_u16, n_left_parser.by_ref()).map(|(first, n_left)| first..=(first + n_left));
            let mut ranges: Vec<RangeInclusive<Sid>> = vec![];
            loop {
                match (n_glyphs as usize).cmp(&covers(&ranges[..])) {
                    std::cmp::Ordering::Equal => return Ok(ranges),
                    std::cmp::Ordering::Greater => ranges.push(parse_item.parse_next(buf)?),
                    std::cmp::Ordering::Less => fail.parse_next(buf)?,
                }
            }
        }
    }

    let n_glyphs = n_glyphs - 1; // 0 is always .notdef, not exist in charsets
    dispatch! {any;
        0 => repeat(n_glyphs as usize,  be_u16).map(Charsets::Format0),
        1 => range_parser(n_glyphs,  be_u8::<&'a [u8], ParserError>.output_into()).map(Charsets::Format1),
        2 => range_parser(n_glyphs,  be_u16).map(Charsets::Format2),
        _ => fail,
    }
}

/// Supplemental data for encoding, replace some char code for a new glyph name.
/// `code` is char code to replace,
/// `sid` is SID of glyph name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EncodingSupplement {
    code: u8,
    sid: Sid,
}

impl EncodingSupplement {
    fn new(code: u8, sid: Sid) -> Self {
        Self { code, sid }
    }

    pub fn apply(self, strings: StringIndex<'_>, encodings: &mut Encoding) {
        encodings[self.code as usize] = name(&strings.get(self.sid));
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EncodingRange {
    first: u8,
    n_left: u8,
}

impl EncodingRange {
    fn new(first: u8, n_left: u8) -> Self {
        Self { first, n_left }
    }
}

/// Encodings map char code to gid, use Charset to map gid to SID.
#[derive(Debug, PartialEq)]
pub enum Encodings {
    Format0(Vec<u8>),
    Format1(Vec<EncodingRange>),
    PredefinedStandard,
    PredefinedExpert,
}

impl Encodings {
    /// build encodings.
    pub fn build(&self, charsets: &Charsets, string_index: StringIndex<'_>) -> Encoding {
        const NOTDEF: Name = sname(prescript::NOTDEF);
        match self {
            Self::Format0(codes) => {
                let mut encodings = [NOTDEF; 256];
                for (i, code) in codes.iter().enumerate() {
                    let gid = (i + 1).try_into();
                    let gid = match gid {
                        Ok(gid) => Some(gid),
                        Err(e) => {
                            #[cfg(debug_assertions)]
                            panic!("Error converting index to gid: {:?}", e);
                            #[cfg(not(debug_assertions))]
                            {
                                log::error!("Error converting index to gid: {:?}", e);
                                None
                            }
                        }
                    };
                    let sid = gid.and_then(|gid| charsets.resolve_sid(gid));
                    if let Some(v) = sid.map(|sid| string_index.get(sid)) {
                        encodings[*code as usize] = name(&v);
                    }
                }
                Encoding::new(encodings)
            }
            Self::Format1(ranges) => {
                let mut encodings = [NOTDEF; 256];
                for range in ranges {
                    for i in range.first..=range.first + range.n_left {
                        if let Some(v) = charsets.resolve_sid(i).map(|sid| string_index.get(sid)) {
                            encodings[i as usize] = name(&v);
                        }
                    }
                }
                Encoding::new(encodings)
            }
            Self::PredefinedStandard => predefined_encodings::STANDARD,
            Self::PredefinedExpert => predefined_encodings::EXPERT,
        }
    }
}

/// Parses Encodings for Format0 and Format1, other predfined encodings are
/// handled by `TopDict::encodings()`.
///
/// First byte lower 7-bits to determinate Format0 or Format1.
///
/// If first byte is 0, then Format0, followed by nCodes (u8) and code (u8) array.
/// If first byte is 1, then Format1, followed by nRanges (u8) and EncodingRange array,
///
/// If first byte highest bit is 1, EncodingSuppliments exists after Format0 or Format 1.
/// EncodingSuppliments is a sequence of code (u8) and sid (u16) preceeded with `nSups` (u8),
/// which is the count of EncodingSuppliment.
fn encodings_parser<'a>()
-> impl Parser<&'a [u8], (Encodings, Option<Vec<EncodingSupplement>>), ParserError> {
    let mut format0 = length_take(be_u8).map(|v: &[u8]| Encodings::Format0(v.to_owned()));
    let mut format1 = length_repeat(
        be_u8,
        (be_u8, be_u8).map(|(first, n_left)| EncodingRange::new(first, n_left)),
    )
    .map(Encodings::Format1);
    let supplement_parser = (be_u8, be_u16).map(|(code, sid)| EncodingSupplement::new(code, sid));
    let mut supplements_parser = length_repeat(be_u8, supplement_parser).map(Some);
    dispatch! { be_u8;
        0 => (format0.by_ref(), empty.value(None)),
        1 => (format1.by_ref(), empty.value(None)),
        0x80 => (format0.by_ref(),  supplements_parser.by_ref()),
        0x81 => (format1.by_ref(),  supplements_parser.by_ref()),
        _ => fail,
    }
}

pub fn parse_fonts(buf: &[u8]) -> Result<(NameIndex<'_>, TopDictIndex<'_>, StringIndex<'_>)> {
    parse_ignore_rest(
        (
            name_index_parser(),
            top_dict_index_parser(),
            string_index_parser(),
        ),
        buf,
    )
    .context(ParseSnafu {
        message: "parse fonts file".to_owned(),
    })
}

#[cfg(test)]
mod tests;
