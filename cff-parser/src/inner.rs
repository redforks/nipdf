use std::{
    collections::HashMap,
    hash::Hash,
    ops::{Deref, Range, RangeInclusive},
    str::from_utf8,
};

use nom::{
    bits::{bits, complete::tag as bit_tag, complete::take as bit_take},
    branch::alt,
    bytes::complete::take,
    combinator::{cond, fail, iterator},
    error::Error as NomError,
    multi::{count, length_count, many1, many_till},
    number::complete::{be_u16, be_u8},
    sequence::pair,
    IResult, Parser,
};
use paste::paste;
use thiserror::Error as ThisError;

mod predefined_charsets;
mod predefined_encodings;

pub type ParseResult<'a, O> = IResult<&'a [u8], O>;

/// String ID, resolve &str from `StringIndex`.
type SID = u16;

/// Glyph ID
type GID = u8;

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

fn parse_integer(buf: &[u8]) -> ParseResult<i32> {
    let (buf, b0) = take(1usize)(buf)?;
    let b0 = b0[0];
    if b0 >= 32 && b0 <= 246 {
        Ok((buf, (b0 as i32) - 139))
    } else if b0 >= 247 && b0 <= 250 {
        let (buf, b1) = take(1usize)(buf)?;
        let b1 = b1[0];
        Ok((buf, ((b0 as i32) - 247) * 256 + (b1 as i32) + 108))
    } else if b0 >= 251 && b0 <= 254 {
        let (buf, b1) = take(1usize)(buf)?;
        let b1 = b1[0];
        Ok((buf, -((b0 as i32) - 251) * 256 - (b1 as i32) - 108))
    } else if b0 == 28 {
        let (buf, b1) = take(1usize)(buf)?;
        let b1 = b1[0];
        let (buf, b2) = take(1usize)(buf)?;
        let b2 = b2[0];
        Ok((buf, (((b1 as i16) << 8) | (b2 as i16)) as i32))
    } else if b0 == 29 {
        let (buf, b1) = take(1usize)(buf)?;
        let b1 = b1[0];
        let (buf, b2) = take(1usize)(buf)?;
        let b2 = b2[0];
        let (buf, b3) = take(1usize)(buf)?;
        let b3 = b3[0];
        let (buf, b4) = take(1usize)(buf)?;
        let b4 = b4[0];
        Ok((
            buf,
            ((b1 as i32) << 24) + ((b2 as i32) << 16) + ((b3 as i32) << 8) + (b4 as i32),
        ))
    } else {
        fail(buf)
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
fn parse_real(buf: &[u8]) -> ParseResult<f32> {
    #[derive(PartialEq, Eq, Debug)]
    enum NumberState {
        Int,
        Mantissa,
        Exponent,
    }

    let (buf, b0) = take(1usize)(buf)?;
    let b0 = b0[0];
    if b0 == 30 {
        let mut sign = 1.0;
        let mut exponent = 0.0;
        let mut int = 0.0;
        let mut mantissa = 0.0;
        let mut mantissa_len = 0;
        let mut exponent_sign = 1.0;
        let mut number_state = NumberState::Int;
        // TODO: rewrite use nom::combinator::iterator()
        let mut parse_nibbles = bits::<_, _, NomError<(&[u8], usize)>, NomError<&[u8]>, _>(
            many_till(bit_take::<_, u8, _, _>(4usize), bit_tag(0xf, 4usize)),
        );
        let (buf, (nibbles, _)) = parse_nibbles(buf)?;
        for nibble in nibbles {
            match nibble {
                0..=9 => match number_state {
                    NumberState::Int => {
                        int = int * 10.0 + (nibble as f32);
                    }
                    NumberState::Mantissa => {
                        mantissa = mantissa * 10.0 + (nibble as f32);
                        mantissa_len += 1;
                    }
                    NumberState::Exponent => {
                        exponent = exponent * 10.0 + (nibble as f32);
                    }
                },
                0xa => {
                    debug_assert_eq!(NumberState::Int, number_state);
                    number_state = NumberState::Mantissa;
                }
                0xb => {
                    number_state = NumberState::Exponent;
                }
                0xc => {
                    number_state = NumberState::Exponent;
                    exponent_sign = -1.0;
                }
                0xe => {
                    sign = -1.0;
                }
                0xf => {
                    break;
                }
                _ => {
                    return fail(buf);
                }
            }
        }
        let mut r =
            int + mantissa * 10f32.powi(-mantissa_len) * 10f32.powf(exponent * exponent_sign);
        r *= sign;
        Ok((buf, r))
    } else {
        fail(buf)
    }
}

/// Operand maybe integer/real/bool/intArray/realArray, if multiple operands
/// are provided, item types must be same, either int or real, returned as
/// intArray/realArray.
fn parse_operand(buf: &[u8]) -> ParseResult<Operand> {
    let (buf, mut values) = many1(alt((
        parse_integer.map(|v| Operand::Integer(v)),
        parse_real.map(|v| Operand::Real(v)),
    )))(buf)?;

    // If values has one item, return it directly.
    if values.len() == 1 {
        return Ok((buf, values.pop().unwrap()));
    }

    // check if all items are same type.
    let mut is_same_type = true;
    let mut is_int = false;
    let mut is_real = false;
    for v in &values {
        match v {
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
            let mut int_array = Vec::with_capacity(values.len());
            for v in values {
                match v {
                    Operand::Integer(i) => int_array.push(i),
                    _ => unreachable!(),
                }
            }
            Ok((buf, Operand::IntArray(int_array)))
        } else if is_real {
            let mut real_array = Vec::with_capacity(values.len());
            for v in values {
                match v {
                    Operand::Real(r) => real_array.push(r),
                    _ => unreachable!(),
                }
            }
            Ok((buf, Operand::RealArray(real_array)))
        } else {
            unreachable!()
        }
    } else {
        unreachable!()
    }
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
    pub const fn new(tag: u8) -> Self {
        debug_assert!(tag <= 21);
        Self { tag, escape: false }
    }

    pub const fn escaped(tag: u8) -> Self {
        Self { tag, escape: true }
    }

    pub const VERSION: Self = Self::new(0);
    pub const NOTICE: Self = Self::new(1);
    pub const COPYRIGHT: Self = Self::escaped(0);
    pub const FULL_NAME: Self = Self::new(2);
    pub const FAMILY_NAME: Self = Self::new(3);
    pub const WEIGHT: Self = Self::new(4);

    pub const IS_FIXED_PITCH: Self = Self::escaped(1);
    pub const ITALIC_ANGLE: Self = Self::escaped(2);
    pub const UNDERLINE_POSITION: Self = Self::escaped(3);
    pub const UNDERLINE_THICKNESS: Self = Self::escaped(4);

    pub const PAINT_TYPE: Self = Self::escaped(5);
    pub const CHARSTRING_TYPE: Self = Self::escaped(6);
    pub const FONT_MATRIX: Self = Self::escaped(7);
    pub const UNIQUE_ID: Self = Self::new(13);
    pub const FONT_BBOX: Self = Self::new(5);
    pub const STROKE_WIDTH: Self = Self::escaped(8);
    pub const XUID: Self = Self::new(14);
    pub const CHARSETS: Self = Self::new(15);
    pub const ENCODINGS: Self = Self::new(16);
    pub const CHAR_STRINGS: Self = Self::new(17);
    pub const PRIVATE: Self = Self::new(18);
    pub const SYNTHETIC_BASE: Self = Self::escaped(20);
    pub const POST_SCRIPT: Self = Self::escaped(21);
    pub const BASE_FONT_NAME: Self = Self::escaped(22);
    pub const BASE_FONT_BLEND: Self = Self::escaped(23);
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

fn parse_operator(buf: &[u8]) -> ParseResult<Operator> {
    let (buf, b0) = take(1usize)(buf)?;
    let b0 = b0[0];
    if b0 == 12 {
        let (buf, b1) = take(1usize)(buf)?;
        let b1 = b1[0];
        Ok((buf, Operator::escaped(b1)))
    } else {
        Ok((buf, Operator::new(b0)))
    }
}

/// Error may returned in this crate.
#[derive(PartialEq, Eq, Debug, Clone, ThisError)]
pub enum Error {
    #[error("Dict value not Integer")]
    ExpectInt,
    #[error("Dict value not Real")]
    ExpectReal,
    #[error("Dict value not Integer Array")]
    ExpectIntArray,
    #[error("Dict value not Real Array")]
    ExpectRealArray,
    #[error("Dict value not Bool")]
    ExpectBool,

    #[error("Invalid offsets data")]
    InvalidOffsetsData,

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Required top dict value missing")]
    RequiredDictValueMissing,
}

impl<E: std::fmt::Debug> From<nom::Err<E>> for Error {
    fn from(e: nom::Err<E>) -> Self {
        Self::ParseError(format!("{}", e))
    }
}

pub type Result<T> = std::result::Result<T, Error>;

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
        self.0.get(&k).map(|v| f(v)).transpose()
    }

    /// If value not exist for `k`, return default value `dv`,
    /// use `f` to convert Operand value to type `T` and returns otherwise.
    fn opt_or<'a, T: 'a, F: FnOnce(&'a Operand) -> Result<T>>(
        &'a self,
        f: F,
        k: Operator,
        dv: T,
    ) -> Result<T> {
        self.0.get(&k).map(|v| f(v)).unwrap_or(Ok(dv))
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
            .map_or(Err(Error::RequiredDictValueMissing), |v| f(&v))
    }

    /// Assume the operand value is delta-encoded, return decoded real number array.
    pub fn as_delta_encoded(&self, k: Operator) -> Result<Option<Vec<f32>>> {
        let r = self.as_real_array(k)?;
        Ok(r.map(|v| {
            let mut r = Vec::with_capacity(v.len());
            let mut prev = 0.0;
            for &i in v {
                r.push(i + prev);
                prev = i + prev;
            }
            r
        }))
    }

    /// Assume the operand value is delta-encoded, return decoded real number array.
    /// Return default value if value not exist.
    pub fn as_delta_encoded_or(&self, k: Operator, default: &'static [f32]) -> Result<Vec<f32>> {
        self.as_delta_encoded(k)
            .map(|v| v.unwrap_or(default.to_vec()))
    }

    pub fn delta_encoded(&self, k: Operator) -> Result<Vec<f32>> {
        self.as_delta_encoded(k)
            .and_then(|v| v.ok_or(Error::RequiredDictValueMissing))
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
    access_methods!(int, |v| v.int().ok_or(Error::ExpectInt), i32);
    access_methods!(real, |v| v.real().ok_or(Error::ExpectReal), f32);
    access_methods!(bool, |v| v.bool().ok_or(Error::ExpectBool), bool);
    access_methods!(
        int_array,
        |v| v.int_array().ok_or(Error::ExpectIntArray),
        &[i32],
        &'static [i32]
    );
    access_methods!(
        real_array,
        |v| v.real_array().ok_or(Error::ExpectRealArray),
        &[f32],
        &'static [f32]
    );
}

/// Parse Dict.
/// Dict stored as a sequence of operators and operands. The operands are
/// stored before the operators.
fn parse_dict(buf: &[u8]) -> ParseResult<Dict> {
    let parse_item = pair(parse_operand, parse_operator).map(|(v, k)| (k, v));
    let (buf, items) = many1(parse_item)(buf)?;
    let dict = items.into_iter().collect();
    Ok((buf, Dict(dict)))
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
    pub fn len(&self) -> usize {
        *self as usize
    }
}

fn parse_off_size(buf: &[u8]) -> ParseResult<OffSize> {
    let (buf, b0) = take(1usize)(buf)?;
    let b0 = b0[0];
    match b0 {
        1 => Ok((buf, OffSize::One)),
        2 => Ok((buf, OffSize::Two)),
        3 => Ok((buf, OffSize::Three)),
        4 => Ok((buf, OffSize::Four)),
        _ => fail(buf),
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
        let (_, first) = Self::_get(data, off_size, 0)
            .map_err(|e| Error::ParseError(format!("parse first offset failed: {:?}", e)))?;
        if first != 1 {
            return Err(Error::InvalidOffsetsData);
        }
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
        let (_, r) = Self::_get(self.1, self.0, ith)
            .unwrap_or_else(|e| panic!("parse offset failed: {:?}", e));
        r as usize - 1
    }

    fn offset_parser<'b>(off_size: OffSize) -> impl Parser<&'b [u8], u32, NomError<&'b [u8]>> {
        use nom::number::complete::{be_u16, be_u24, be_u32, be_u8};
        move |buf| -> ParseResult<u32> {
            match off_size {
                OffSize::One => be_u8.map(|v| v as u32).parse(buf),
                OffSize::Two => be_u16.map(|v| v as u32).parse(buf),
                OffSize::Three => be_u24.map(|v| v as u32).parse(buf),
                OffSize::Four => be_u32(buf),
            }
        }
    }

    /// Get offset of `ith` element
    fn _get(data: &[u8], off_size: OffSize, ith: usize) -> ParseResult<u32> {
        // skip ith off_size bytes
        let buf = &data[ith * off_size.len()..];
        Self::offset_parser(off_size).parse(buf)
    }
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

    /// Get value by index. Using parser `f` to decode data.
    /// Panic if `idx` is out of range.
    pub fn get<T: 'a, F: Parser<&'a [u8], T, NomError<&'a [u8]>>>(
        &self,
        idx: usize,
        mut f: F,
    ) -> Result<T> {
        let range = self.offsets.range(idx);
        let buf = &self.data[range];
        f.parse(buf)
            .map_err(|e| {
                log::error!("parse data failed: {:?}", e);
                Error::ParseError(format!("parse data failed: {:?}", e))
            })
            .map(|(_, v)| v)
    }

    /// Get str by index. Panic if `idx` is out of range.
    /// Returns `&[u8]` instead of `&str`, because the str may not be valid utf8,
    /// `from_utf8()` returns error if str contains '\0'.
    pub fn get_bin_str(&self, idx: usize) -> &'a [u8] {
        fn parse_name(buf: &[u8]) -> ParseResult<'_, &'_ [u8]> {
            Ok((&buf[0..0], &buf[..]))
        }

        self.get(idx, parse_name).unwrap()
    }

    /// Get Dict by index. Panic if `idx` is out of range.
    pub fn get_dict(&self, idx: usize) -> Dict {
        self.get(idx, parse_dict).unwrap()
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
pub fn parse_indexed_data(buf: &[u8]) -> ParseResult<IndexedData<'_>> {
    let (buf, n) = be_u16(buf)?;
    let (buf, off_size) = parse_off_size(buf)?;

    let offset_data_len = (n + 1) as usize * off_size.len();
    let (buf, offset_data) = take(offset_data_len)(buf)?;
    let offsets = Offsets::new(off_size, offset_data).map_err(|e| {
        log::error!("parse offsets failed: {:?}", e);
        nom::Err::Error(NomError::new(buf, nom::error::ErrorKind::Verify))
    })?;

    let data_len = offsets.get(n as usize);
    let (buf, data) = take(data_len)(buf)?;

    Ok((buf, IndexedData { offsets, data }))
}

pub fn parse_name_index(buf: &[u8]) -> ParseResult<NameIndex<'_>> {
    parse_indexed_data.map(NameIndex).parse(buf)
}

pub fn parse_string_index(buf: &[u8]) -> ParseResult<StringIndex<'_>> {
    parse_indexed_data.map(StringIndex).parse(buf)
}

pub fn parse_top_dict_index(buf: &[u8]) -> ParseResult<TopDictIndex<'_>> {
    parse_indexed_data.map(TopDictIndex).parse(buf)
}

/// Header of CFF.
#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub struct Header {
    pub major: u8,
    pub minor: u8,
    pub hdr_size: u8,
    pub off_size: OffSize,
}

pub fn parse_header(buf: &[u8]) -> ParseResult<Header> {
    let (buf, major) = take(1usize)(buf)?;
    let major = major[0];
    let (buf, minor) = take(1usize)(buf)?;
    let minor = minor[0];
    let (buf, hdr_size) = take(1usize)(buf)?;
    let hdr_size = hdr_size[0];
    let (buf, off_size) = parse_off_size(buf)?;
    Ok((
        buf,
        Header {
            major,
            minor,
            hdr_size,
            off_size,
        },
    ))
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
    pub fn get(&self, idx: usize) -> Option<&'a str> {
        let name = self.0.get_bin_str(idx);
        if name.is_empty() {
            None
        } else {
            if name[0] == 0 {
                None
            } else {
                Some(from_utf8(name).unwrap())
            }
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
    pub fn get(&self, idx: SID) -> &'a str {
        if idx < 391 {
            STANDARD_STRINGS[idx as usize]
        } else {
            from_utf8(self.0.get_bin_str((idx - 391) as usize)).unwrap()
        }
    }
}

/// Standard strings defined in CFF spec, used in Type 1 and some other strings.
#[rustfmt::skip]
const STANDARD_STRINGS: [&'static str; 391] = [
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
    strings: StringIndex<'a>,
    dict: Dict,
}

/// SIDDict deref to Dict, to add Dict access methods.
impl<'a> Deref for SIDDict<'a> {
    type Target = Dict;

    fn deref(&self) -> &Self::Target {
        &self.dict
    }
}

impl<'a> SIDDict<'a> {
    fn resolve_sid(&self, v: &Operand) -> Result<&str> {
        v.int()
            .ok_or(Error::ExpectInt)
            .map(|v| self.strings.get(v as SID))
    }

    pub fn sid(&self, k: Operator) -> Result<&str> {
        self.required(|v| self.resolve_sid(v), k)
    }

    pub fn as_sid(&self, k: Operator) -> Result<Option<&str>> {
        self.opt(|v| self.resolve_sid(v), k)
    }

    pub fn as_sid_or(&self, k: Operator, default: &'static str) -> Result<&str> {
        self.opt_or(|v| self.resolve_sid(v), k, default)
    }
}

/// Top Dict for each font face.
#[derive(Debug)]
pub struct TopDictData<'a>(SIDDict<'a>);

impl<'a> TopDictData<'a> {
    pub fn new(strings: StringIndex<'a>, dict: Dict) -> Self {
        Self(SIDDict { strings, dict })
    }

    pub fn string_index(&self) -> StringIndex<'a> {
        self.0.strings
    }

    pub fn version(&self) -> Result<&str> {
        self.0.sid(Operator::VERSION)
    }

    pub fn notice(&self) -> Result<&str> {
        self.0.sid(Operator::NOTICE)
    }

    pub fn copyright(&self) -> Result<&str> {
        self.0.sid(Operator::COPYRIGHT)
    }

    pub fn full_name(&self) -> Result<&str> {
        self.0.sid(Operator::FULL_NAME)
    }

    pub fn family_name(&self) -> Result<&str> {
        self.0.sid(Operator::FAMILY_NAME)
    }

    pub fn weight(&self) -> Result<&str> {
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
            _ => Ok(parse_charsets(&file[offset as usize..], self.n_glyphs(file)?)?.1),
        }
    }

    pub fn encodings(&self, file: &[u8]) -> Result<(Encodings, Option<Vec<EncodingSupplement>>)> {
        let offset = self.0.as_int_or(Operator::ENCODINGS, 0)?;

        match offset {
            0 => Ok((Encodings::PredefinedStandard, None)),
            1 => Ok((Encodings::PredefinedExpert, None)),
            _ => Ok(parse_encodings(&file[offset as usize..])?.1),
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
        let (_, index) = parse_indexed_data(buf)?;
        Ok(index.len() as u16)
    }

    pub fn synthetic_base(&self) -> Result<i32> {
        self.0.int(Operator::SYNTHETIC_BASE)
    }

    pub fn post_script(&self) -> Result<&str> {
        self.0.sid(Operator::POST_SCRIPT)
    }

    pub fn base_font_name(&self) -> Result<&str> {
        self.0.sid(Operator::BASE_FONT_NAME)
    }

    pub fn base_font_blend(&self) -> Result<Vec<f32>> {
        self.0.delta_encoded(Operator::BASE_FONT_BLEND)
    }
}

/// IndexedData to store TopDicts. Each item is TopDict
pub struct TopDictIndex<'a>(IndexedData<'a>);

impl<'a> TopDictIndex<'a> {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn get(&self, idx: usize, strings: StringIndex<'a>) -> Result<TopDictData<'a>> {
        let parse = parse_dict.map(|v| TopDictData::new(strings, v));
        self.0.get(idx, parse)
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PredefinedCharsets {
    ISOAdobe = 0,
    Expert = 1,
    ExpertSubset = 2,
}

/// Charsets map code index (u8) to SID
#[derive(Debug, PartialEq)]
pub enum Charsets {
    Format0(Vec<SID>),
    Format1(Vec<RangeInclusive<SID>>), // (first, n_left: u8)
    Format2(Vec<RangeInclusive<SID>>), // (first, n_left: u16)
    Predefined(PredefinedCharsets),
}

impl Charsets {
    /// Return SID by index. Return None if `idx` is out of range.
    pub fn resolve_sid(&self, idx: usize) -> Option<SID> {
        match self {
            Self::Predefined(predefined) => match predefined {
                PredefinedCharsets::ISOAdobe => (idx < 229).then_some(idx as SID),
                PredefinedCharsets::Expert => predefined_charsets::EXPERT.get(idx).copied(),
                PredefinedCharsets::ExpertSubset => {
                    predefined_charsets::EXPERT_SUBSET.get(idx).copied()
                }
            },
            _ => todo!(),
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
fn parse_charsets(buf: &[u8], n_glyphs: u16) -> ParseResult<Charsets> {
    let n_glyphs = n_glyphs - 1; // 0 is always .notdef, not exist in charsets

    fn covers(r: &[RangeInclusive<SID>]) -> usize {
        let mut covers = 0;
        for range in r {
            covers += range.len();
        }
        covers
    }

    fn range_parser<
        'a,
        N: Into<u16> + Sized,
        E: nom::error::ParseError<&'a [u8]>,
        P: Parser<&'a [u8], N, E>,
    >(
        n_glyphs: u16,
        n_left_parser: P,
    ) -> impl Parser<&'a [u8], Vec<RangeInclusive<SID>>, E> {
        let mut parse_item =
            pair(be_u16, n_left_parser).map(|(first, n_left)| first..=(first + n_left.into()));
        move |buf| {
            let mut ranges = vec![];
            let mut iter = iterator(buf, |buf| parse_item.parse(buf));
            iter.map_while(|v| {
                ranges.push(v);
                match n_glyphs as i32 - covers(&ranges[..]) as i32 {
                    0 => None,
                    1.. => Some(()),
                    ..=-1 => panic!("parse charsets failed: {:?}", ranges.last().unwrap()),
                }
            })
            .for_each(|_| ());
            Ok((iter.finish()?.0, ranges))
        }
    }

    let (buf, format) = take(1usize)(buf)?;
    let format = format[0];
    match format {
        0 => {
            let (buf, sids) = count(be_u16, n_glyphs as usize)(buf)?;
            Ok((buf, Charsets::Format0(sids)))
        }
        1 => range_parser(n_glyphs, be_u8)
            .map(Charsets::Format1)
            .parse(buf),
        2 => range_parser(n_glyphs, be_u16)
            .map(Charsets::Format2)
            .parse(buf),

        _ => fail(buf),
    }
}

/// Supplemental data for encoding, replace some char code for a new glyph name.
/// `code` is char code to replace,
/// `sid` is SID of glyph name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EncodingSupplement {
    code: u8,
    sid: SID,
}

impl EncodingSupplement {
    fn new(code: u8, sid: SID) -> Self {
        Self { code, sid }
    }

    pub fn apply<'a>(&self, string_index: StringIndex<'a>, encodings: &mut [Option<&'a str>; 256]) {
        encodings[self.code as usize] = Some(string_index.get(self.sid));
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

#[derive(Debug, PartialEq)]
pub enum Encodings {
    Format0(Vec<u8>),
    Format1(Vec<EncodingRange>),
    PredefinedStandard,
    PredefinedExpert,
}

impl Encodings {
    /// build encodings.
    pub fn build<'a>(
        &self,
        charsets: &Charsets,
        string_index: StringIndex<'a>,
    ) -> [Option<&'a str>; 256] {
        match self {
            Self::Format0(codes) => {
                let mut encodings = [None; 256];
                for (i, code) in codes.iter().enumerate() {
                    encodings[*code as usize] =
                        charsets.resolve_sid(i).map(|sid| string_index.get(sid));
                }
                encodings
            }
            Self::PredefinedStandard => predefined_encodings::STANDARD,
            Self::PredefinedExpert => predefined_encodings::EXPERT,
            _ => todo!(),
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
fn parse_encodings(buf: &[u8]) -> ParseResult<(Encodings, Option<Vec<EncodingSupplement>>)> {
    let (buf, format) = be_u8(buf)?;
    let (buf, encodings) = match format & 0x7f {
        0 => length_count(be_u8, be_u8)
            .map(Encodings::Format0)
            .parse(buf)?,
        1 => {
            let range_parser =
                pair(be_u8, be_u8).map(|(first, n_left)| EncodingRange { first, n_left });
            length_count(be_u8, range_parser)
                .map(Encodings::Format1)
                .parse(buf)?
        }
        _ => fail(buf)?,
    };
    let supplement_parser = pair(be_u8, be_u16).map(|(code, sid)| EncodingSupplement { code, sid });
    let mut supplements_parser = length_count(be_u8, supplement_parser);
    let (buf, supplements) = cond(format & 0x80 != 0, supplements_parser)(buf)?;

    Ok((buf, (encodings, supplements)))
}

#[cfg(test)]
mod tests;
