//! Cmap to map CharCode to CID, used in Type0/CID font

use crate::{
    machine::{
        ok, Key, Machine, MachineError, MachinePlugin, MachineResult,
        RuntimeDictionary, RuntimeValue,
    },
    sname, Name,
};
use educe::Educe;
use either::Either::{self, Right};
use log::error;
use std::{collections::HashMap, rc::Rc, str::from_utf8, marker::PhantomData};
use tinyvec::ArrayVec;
use phf::phf_map;
use once_cell::unsync::OnceCell;

/// Convert from CharCode using cmap, use it to select glyph id
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CID(pub u16);

/// Input code type, can be one/two/three/four bytes.
/// TODO: bytes in any length
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharCode {
    One(u8),
    Two(u8, u8),
    Three(u8, u8, u8),
    Four(u8, u8, u8, u8),
}

impl CharCode {
    fn from_str_buf(s: &[u8]) -> Self {
        match s.len() {
            1 => Self::One(s[0]),
            2 => Self::Two(s[0], s[1]),
            3 => Self::Three(s[0], s[1], s[2]),
            4 => Self::Four(s[0], s[1], s[2], s[3]),
            _ => panic!("invalid bytes length"),
        }
    }

    pub fn n_bytes(&self) -> usize {
        match self {
            Self::One(_) => 1,
            Self::Two(_, _) => 2,
            Self::Three(_, _, _) => 3,
            Self::Four(_, _, _, _) => 4,
        }
    }
}

fn parse_cid_from_str_buf(s: &[u8]) -> CID {
    let bytes = match s.len() {
        1 => [0, s[0]],
        2 => [s[0], s[1]],
        _ => unreachable!(),
    };
    CID(u16::from_be_bytes(bytes))
}

impl AsRef<[u8]> for CharCode {
    fn as_ref(&self) -> &[u8] {
        use std::slice::from_raw_parts;
        match self {
            Self::One(b) => std::slice::from_ref(b),
            Self::Two(b1, _) => unsafe { from_raw_parts(b1, 2) },
            Self::Three(b1, _, _) => unsafe { from_raw_parts(b1, 3) },
            Self::Four(b1, _, _, _) => unsafe { from_raw_parts(b1, 4) },
        }
    }
}

impl From<&[u8]> for CharCode {
    fn from(bytes: &[u8]) -> Self {
        match bytes.len() {
            1 => Self::One(bytes[0]),
            2 => Self::Two(bytes[0], bytes[1]),
            3 => Self::Three(bytes[0], bytes[1], bytes[2]),
            4 => Self::Four(bytes[0], bytes[1], bytes[2], bytes[3]),
            _ => panic!("invalid bytes length"),
        }
    }
}

/// Common trait that maps CharCode to CID.
trait CodeMap {
    fn map(&self, code: CharCode) -> Option<CID>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct ByteRange {
    lower: u8,
    upper: u8,
}

impl ByteRange {
    pub fn new(lower: u8, upper: u8) -> Self {
        assert!(lower <= upper);
        Self { lower, upper }
    }

    fn in_range(&self, c: u8) -> bool {
        self.lower <= c && c <= self.upper
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodeSpaceResult {
    /// Matched
    Matched(CharCode),
    /// Partial match, not enough bytes, or prefix bytes matched.
    Partial(CharCode),
    /// No match
    NotMatched,
}

/// A range entry in code space, lower and upper must have the same length.
/// A range matches N bytes, N is the length of inner array, each item
/// defines a range of bytes, first item for first byte, second item for
/// second byte, and so on.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CodeRange(ArrayVec<[ByteRange; 4]>);

impl CodeRange {
    fn from_str_buf(lower: &[u8], upper: &[u8]) -> Option<Self> {
        if lower.len() != upper.len() {
            return None;
        }
        let mut r = ArrayVec::new();
        for (l, u) in lower.iter().copied().zip(upper.iter().copied()) {
            r.push(ByteRange::new(l, u));
        }
        Some(Self(r))
    }

    /// If ch not in range, return None,
    /// else return offset from lower bound.
    fn offset(&self, ch: CharCode) -> Option<u16> {
        if ch.n_bytes() != self.n_bytes() {
            return None;
        }

        let mut offset = 0u16;
        for (r, c) in self.0.iter().zip(ch.as_ref().iter().copied()) {
            if !r.in_range(c) {
                return None;
            }
            offset = offset * (r.upper as u16 - r.lower as u16 + 1) + (c as u16 - r.lower as u16);
        }
        Some(offset)
    }

    /// `ch` in this range if: ch has same length as range, and each byte in nth byte range.
    fn in_range(&self, ch: CharCode) -> bool {
        self.offset(ch).is_some()
    }

    /// Find next code.
    fn next_code(&self, codes: &[u8]) -> CodeSpaceResult {
        match self
            .0
            .iter()
            .zip(codes.iter().copied())
            .take_while(|(r, c)| r.in_range(*c))
            .count()
        {
            0 => CodeSpaceResult::NotMatched,
            n if n == self.n_bytes() => CodeSpaceResult::Matched(CharCode::from(&codes[..n])),
            _ => {
                CodeSpaceResult::Partial(CharCode::from(&codes[..self.n_bytes().min(codes.len())]))
            }
        }
    }

    fn n_bytes(&self) -> usize {
        self.0.len()
    }
}

/// CodeSpace made up by CodeRanges.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct CodeSpace(Box<[CodeRange]>);

impl CodeSpace {
    fn new(ranges: Vec<CodeRange>) -> Self {
        Self(ranges.into_boxed_slice())
    }

    /// Take next code from input codes, return the rest codes and the next code.
    /// If next code not in code space, return `Left(next_code)`.
    /// Returns minimal bytes of current CodeSpace, even in error cases, append zero if not
    /// enough bytes.
    /// Panic if input codes is empty.
    fn next_code<'a>(&self, codes: &'a [u8]) -> (&'a [u8], Either<CharCode, CharCode>) {
        let next = self
            .0
            .iter()
            .find_map(|r| {
                let r = r.next_code(codes);
                match r {
                    CodeSpaceResult::Matched(code) => Some(Either::Right(code)),
                    CodeSpaceResult::Partial(code) => Some(Either::Left(code)),
                    CodeSpaceResult::NotMatched => None,
                }
            })
            .unwrap_or_else(|| Either::Left(CharCode::One(codes[0])))
            .map_left(|code| {
                let min_bytes = self.min_bytes();
                if code.n_bytes() >= min_bytes {
                    return code;
                }

                let mut bytes = Vec::with_capacity(min_bytes);
                bytes.extend_from_slice(&codes[..min_bytes.min(codes.len())]);
                bytes.resize(min_bytes, 0);
                CharCode::from(bytes.as_slice())
            });
        (&codes[next.into_inner().n_bytes().min(codes.len())..], next)
    }

    fn min_bytes(&self) -> usize {
        self.0
            .iter()
            .map(|r| r.n_bytes())
            .min()
            .expect("Should not happen")
    }
}

/// Maps a range of codes to CID, first code in range map to `start_cid`,
/// 2nd code map to `start_cid + 1`, and so on.
#[derive(Debug, Clone, PartialEq, Eq)]
struct IncRangeMap {
    range: CodeRange,
    start_cid: CID,
}

impl CodeMap for IncRangeMap {
    fn map(&self, code: CharCode) -> Option<CID> {
        self.range
            .offset(code)
            .map(|offset| CID(self.start_cid.0 + offset))
    }
}

impl EntryParse for IncRangeMap {
    fn parse_entry<P>(m: &mut Machine<P>) -> Result<Self, MachineError> {
        let cid = m.pop()?.int()?.try_into().unwrap();
        let s_upper = m.pop()?.string()?;
        let s_lower = m.pop()?.string()?;
        let range = CodeRange::from_str_buf(&s_lower.borrow(), &s_upper.borrow()).ok_or_else(
            || {
                error!("Invalid code range");
                MachineError::TypeCheck
            },
        )?;
        Ok(Self {
            range,
            start_cid: CID(cid),
        })
    }
}

/// Maps a range of codes to CID, all codes in range map to `cid`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RangeMapToOne {
    range: CodeRange,
    cid: CID,
}

impl CodeMap for RangeMapToOne {
    fn map(&self, code: CharCode) -> Option<CID> {
        (self.range.in_range(code)).then_some(self.cid)
    }
}

/// Maps a single code to CID.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SingleCodeMap {
    code: CharCode,
    cid: CID,
}

impl SingleCodeMap {
    fn new(code: CharCode, cid: CID) -> Self {
        Self { code, cid }
    }
}

impl CodeMap for SingleCodeMap {
    fn map(&self, code: CharCode) -> Option<CID> {
        (code == self.code).then_some(self.cid)
    }
}

impl EntryParse for SingleCodeMap {
    fn parse_entry<P>(m: &mut Machine<P>) -> Result<Self, MachineError> {
        let cid = m.pop()?.int()?.try_into().unwrap();
        let s_code = m.pop()?.string()?;
        let code = CharCode::from_str_buf(&s_code.borrow());
        Ok(Self::new(code, CID(cid)))
    }
}

/// Compound mapper that combines range and single code maps.
/// Single Code maps has higher priority than range maps.
#[derive(Debug, Clone, PartialEq, Eq, Educe)]
#[educe(Default)]
struct Mapper<R> {
    ranges: Box<[R]>,
    chars: Box<[SingleCodeMap]>,
}

impl<R: CodeMap> CodeMap for Mapper<R> {
    fn map(&self, code: CharCode) -> Option<CID> {
        let find_in_chars = self.chars.iter().filter_map(|m| m.map(code));
        let find_in_ranges = self.ranges.iter().filter_map(|m| m.map(code));
        find_in_chars.chain(find_in_ranges).next()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Hash)]
pub struct CIDSystemInfo {
    registry: String,
    ordering: String,
    supplement: u16,
}

impl CIDSystemInfo {
    fn from_dict<P>(d: &RuntimeDictionary<P>) -> MachineResult<Self> {
        let registry = from_utf8(&d[&sname("Registry")].string()?.borrow())
            .unwrap()
            .to_owned();
        let ordering = from_utf8(&d[&sname("Ordering")].string()?.borrow())
            .unwrap()
            .to_owned();
        let supplement = d[&sname("Supplement")].int()?.try_into().unwrap();
        Ok(Self {
            registry,
            ordering,
            supplement,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WriteMode {
    #[default]
    Horizontal = 0,
    Vertical = 1,
}

impl WriteMode {
    fn parse(v: i32) -> MachineResult<Self> {
        match v {
            0 => Ok(Self::Horizontal),
            1 => Ok(Self::Vertical),
            _ => {
                error!("Invalid WriteMode: {}", v);
                Err(MachineError::TypeCheck)
            }
        }
    }
}

const GB_EUC_H: &[u8] =include_bytes!("../cmap-resources/Adobe-GB1-6/CMap/GB-EUC-H");
const GB_EUC_V: &[u8] =include_bytes!("../cmap-resources/Adobe-GB1-6/CMap/GB-EUC-V");
const GBPC_EUC_H: &[u8] =include_bytes!("../cmap-resources/Adobe-GB1-6/CMap/GBpc-EUC-H");
const GBPC_EUC_V: &[u8] =include_bytes!("../cmap-resources/Adobe-GB1-6/CMap/GBpc-EUC-V");
const GBK_EUC_V: &[u8] =include_bytes!("../cmap-resources/Adobe-GB1-6/CMap/GBK-EUC-H");
const GBK_EUC_H: &[u8] =include_bytes!("../cmap-resources/Adobe-GB1-6/CMap/GBK-EUC-V");
const GBKP_EUC_V: &[u8] =include_bytes!("../cmap-resources/Adobe-GB1-6/CMap/GBKp-EUC-H");
const GBKP_EUC_H: &[u8] =include_bytes!("../cmap-resources/Adobe-GB1-6/CMap/GBKp-EUC-V");
const GBK2K_H: &[u8] =include_bytes!("../cmap-resources/Adobe-GB1-6/CMap/GBK2K-H");
const GBK2K_V: &[u8] =include_bytes!("../cmap-resources/Adobe-GB1-6/CMap/GBK2K-V");
const UNI_GB_GCSS_H: &[u8] =include_bytes!("../cmap-resources/Adobe-GB1-6/CMap/UniGB-UCS2-H");
const UNI_GB_GCSS_V: &[u8] =include_bytes!("../cmap-resources/Adobe-GB1-6/CMap/UniGB-UCS2-V");
const UNI_GB_UTF16_H: &[u8] =include_bytes!("../cmap-resources/Adobe-GB1-6/CMap/UniGB-UTF16-H");
const UNI_GB_UTF16_V: &[u8] =include_bytes!("../cmap-resources/Adobe-GB1-6/CMap/UniGB-UTF16-V");


const B5PC_H: &[u8] = include_bytes!("../cmap-resources/Adobe-CNS1-7/CMap/B5pc-H");
const B5PC_V: &[u8] = include_bytes!("../cmap-resources/Adobe-CNS1-7/CMap/B5pc-V");
const HKSCS_B5_H: &[u8] = include_bytes!("../cmap-resources/Adobe-CNS1-7/CMap/HKscs-B5-H");
const HKSCS_B5_V: &[u8] = include_bytes!("../cmap-resources/Adobe-CNS1-7/CMap/HKscs-B5-V");
const ETEN_B5_H: &[u8] = include_bytes!("../cmap-resources/Adobe-CNS1-7/CMap/ETen-B5-H");
const ETEN_B5_V: &[u8] = include_bytes!("../cmap-resources/Adobe-CNS1-7/CMap/ETen-B5-V");
const ETENMS_B5_H: &[u8] = include_bytes!("../cmap-resources/Adobe-CNS1-7/CMap/ETenms-B5-H");
const ETENMS_B5_V: &[u8] = include_bytes!("../cmap-resources/Adobe-CNS1-7/CMap/ETenms-B5-V");
const CNS_EUC_H: &[u8] = include_bytes!("../cmap-resources/Adobe-CNS1-7/CMap/CNS-EUC-H");
const CNS_EUC_V: &[u8] = include_bytes!("../cmap-resources/Adobe-CNS1-7/CMap/CNS-EUC-V");
const UNI_CNS_UCS2_H: &[u8] = include_bytes!("../cmap-resources/Adobe-CNS1-7/CMap/UniCNS-UCS2-H");
const UNI_CNS_UCS2_V: &[u8] = include_bytes!("../cmap-resources/Adobe-CNS1-7/CMap/UniCNS-UCS2-V");
const UNI_CNS_UTF16_H: &[u8] = include_bytes!("../cmap-resources/Adobe-CNS1-7/CMap/UniCNS-UTF16-H");
const UNI_CNS_UTF16_V: &[u8] = include_bytes!("../cmap-resources/Adobe-CNS1-7/CMap/UniCNS-UTF16-V");

const _83PV_RKSJ_H: &[u8] = include_bytes!("../cmap-resources/Adobe-Japan1-7/CMap/83pv-RKSJ-H");
const _90MS_RKSJ_H: &[u8] = include_bytes!("../cmap-resources/Adobe-Japan1-7/CMap/90ms-RKSJ-H");
const _90MS_RKSJ_V: &[u8] = include_bytes!("../cmap-resources/Adobe-Japan1-7/CMap/90ms-RKSJ-V");
const _90MSP_RKSJ_H: &[u8] = include_bytes!("../cmap-resources/Adobe-Japan1-7/CMap/90msp-RKSJ-H");
const _90MSP_RKSJ_V: &[u8] = include_bytes!("../cmap-resources/Adobe-Japan1-7/CMap/90msp-RKSJ-V");
const _90PV_RKSJ_H: &[u8] = include_bytes!("../cmap-resources/Adobe-Japan1-7/CMap/90pv-RKSJ-H");
const ADD_RKSJ_H: &[u8] = include_bytes!("../cmap-resources/Adobe-Japan1-7/CMap/Add-RKSJ-H");
const ADD_RKSJ_V: &[u8] = include_bytes!("../cmap-resources/Adobe-Japan1-7/CMap/Add-RKSJ-V");
const EUC_H: &[u8] = include_bytes!("../cmap-resources/Adobe-Japan1-7/CMap/EUC-H");
const EUC_V: &[u8] = include_bytes!("../cmap-resources/Adobe-Japan1-7/CMap/EUC-V");
const EXT_H: &[u8] = include_bytes!("../cmap-resources/Adobe-Japan1-7/CMap/Ext-RKSJ-H");
const EXT_V: &[u8] = include_bytes!("../cmap-resources/Adobe-Japan1-7/CMap/Ext-RKSJ-V");
const H: &[u8] = include_bytes!("../cmap-resources/Adobe-Japan1-7/CMap/H");
const V: &[u8] = include_bytes!("../cmap-resources/Adobe-Japan1-7/CMap/V");
const UNI_JIS_UCS2_H: &[u8] = include_bytes!("../cmap-resources/Adobe-Japan1-7/CMap/UniJIS-UCS2-H");
const UNI_JIS_UCS2_V: &[u8] = include_bytes!("../cmap-resources/Adobe-Japan1-7/CMap/UniJIS-UCS2-V");
const UNI_JIS_UCS2_HW_H: &[u8] = include_bytes!("../cmap-resources/Adobe-Japan1-7/CMap/UniJIS-UCS2-HW-H");
const UNI_JIS_UCS2_HW_V: &[u8] = include_bytes!("../cmap-resources/Adobe-Japan1-7/CMap/UniJIS-UCS2-HW-V");
const UNI_JIS_UTF16_H: &[u8] = include_bytes!("../cmap-resources/Adobe-Japan1-7/CMap/UniJIS-UTF16-H");
const UNI_JIS_UTF16_V: &[u8] = include_bytes!("../cmap-resources/Adobe-Japan1-7/CMap/UniJIS-UTF16-V");

const KSC_EUC_H: &[u8] = include_bytes!("../cmap-resources/Adobe-Korea1-2/CMap/KSC-EUC-H");
const KSC_EUC_V: &[u8] = include_bytes!("../cmap-resources/Adobe-Korea1-2/CMap/KSC-EUC-V");
const KSCMS_UHC_H: &[u8] = include_bytes!("../cmap-resources/Adobe-Korea1-2/CMap/KSCms-UHC-H");
const KSCMS_UHC_V: &[u8] = include_bytes!("../cmap-resources/Adobe-Korea1-2/CMap/KSCms-UHC-V");
const KSCMS_UHC_HW_H: &[u8] = include_bytes!("../cmap-resources/Adobe-Korea1-2/CMap/KSCms-UHC-HW-H");
const KSCMS_UHC_HW_V: &[u8] = include_bytes!("../cmap-resources/Adobe-Korea1-2/CMap/KSCms-UHC-HW-V");
const KSCPC_EUC_H: &[u8] = include_bytes!("../cmap-resources/Adobe-Korea1-2/CMap/KSCpc-EUC-H");
const UNI_KS_UCS2_H: &[u8] = include_bytes!("../cmap-resources/Adobe-Korea1-2/CMap/UniKS-UCS2-H");
const UNI_KS_UCS2_V: &[u8] = include_bytes!("../cmap-resources/Adobe-Korea1-2/CMap/UniKS-UCS2-V");
const UNI_KS_UTF16_H: &[u8] = include_bytes!("../cmap-resources/Adobe-Korea1-2/CMap/UniKS-UTF16-H");
const UNI_KS_UTF16_V: &[u8] = include_bytes!("../cmap-resources/Adobe-Korea1-2/CMap/UniKS-UTF16-V");

const IDENTITY_H: &[u8] = include_bytes!("../cmap-resources/Adobe-Identity-0/CMap/Identity-H");
const IDENTITY_V: &[u8] = include_bytes!("../cmap-resources/Adobe-Identity-0/CMap/Identity-V");
static PREDEFINED_CMAPS: phf::Map<&'static str, &'static [u8]> = phf_map!{
    "GB-EUC-H" => GB_EUC_H,
    "GB-EUC-V" => GB_EUC_V,
    "GBpc-EUC-H" => GBPC_EUC_H,
    "GBpc-EUC-V" => GBPC_EUC_V,
    "GBK-EUC-H" => GBK_EUC_V,
    "GBK-EUC-V" => GBK_EUC_H,
    "GBKp-EUC-H" => GBKP_EUC_V,
    "GBKp-EUC-V" => GBKP_EUC_H,
    "GBK2K-H" => GBK2K_H,
    "GBK2K-V" => GBK2K_V,
    "UniGB-UCS2-H" => UNI_GB_GCSS_H,
    "UniGB-UCS2-V" => UNI_GB_GCSS_V,
    "UniGB-UTF16-H" => UNI_GB_UTF16_H,
    "UniGB-UTF16-V" => UNI_GB_UTF16_V,

    "B5pc-H" => B5PC_H,
    "B5pc-V" => B5PC_V,
    "HKscs-B5-H" => HKSCS_B5_H,
    "HKscs-B5-V" => HKSCS_B5_V,
    "ETen-B5-H" => ETEN_B5_H,
    "ETen-B5-V" => ETEN_B5_V,
    "ETenms-B5-H" => ETENMS_B5_H,
    "ETenms-B5-V" => ETENMS_B5_V,
    "CNS-EUC-H" => CNS_EUC_H,
    "CNS-EUC-V" => CNS_EUC_V,
    "UniCNS-UCS2-H" => UNI_CNS_UCS2_H,
    "UniCNS-UCS2-V" => UNI_CNS_UCS2_V,
    "UniCNS-UTF16-H" => UNI_CNS_UTF16_H,
    "UniCNS-UTF16-V" => UNI_CNS_UTF16_V,

    "83pv-RKSJ-H" => _83PV_RKSJ_H,
    "90ms-RKSJ-H" => _90MS_RKSJ_H,
    "90ms-RKSJ-V" => _90MS_RKSJ_V,
    "90msp-RKSJ-H" => _90MSP_RKSJ_H,
    "90msp-RKSJ-V" => _90MSP_RKSJ_V,
    "90pv-RKSJ-H" => _90PV_RKSJ_H,
    "Add-RKSJ-H" => ADD_RKSJ_H,
    "Add-RKSJ-V" => ADD_RKSJ_V,
    "EUC-H" => EUC_H,
    "EUC-V" => EUC_V,
    "Ext-RKSJ-H" => EXT_H,
    "Ext-RKSJ-V" => EXT_V,
    "H" => H,
    "V" => V,
    "UniJIS-UCS2-H" => UNI_JIS_UCS2_H,
    "UniJIS-UCS2-V" => UNI_JIS_UCS2_V,
    "UniJIS-UCS2-HW-H" => UNI_JIS_UCS2_HW_H,
    "UniJIS-UCS2-HW-V" => UNI_JIS_UCS2_HW_V,
    "UniJIS-UTF16-H" => UNI_JIS_UTF16_H,
    "UniJIS-UTF16-V" => UNI_JIS_UTF16_V,

    "KSC-EUC-H" => KSC_EUC_H,
    "KSC-EUC-V" => KSC_EUC_V,
    "KSCms-UHC-H" => KSCMS_UHC_H,
    "KSCms-UHC-V" => KSCMS_UHC_V,
    "KSCms-UHC-HW-H" => KSCMS_UHC_HW_H,
    "KSCms-UHC-HW-V" => KSCMS_UHC_HW_V,
    "KSCpc-EUC-H" => KSCPC_EUC_H,
    "UniKS-UCS2-H" => UNI_KS_UCS2_H,
    "UniKS-UCS2-V" => UNI_KS_UCS2_V,
    "UniKS-UTF16-H" => UNI_KS_UTF16_H,
    "UniKS-UTF16-V" => UNI_KS_UTF16_V,

    "Identity-H" => IDENTITY_H,
    "Identity-V" => IDENTITY_V,
};

/// CMapRegistry contains all CMaps, access by CMap Name.
#[derive(Debug)]
pub struct CMapRegistry {
    predefined: HashMap<&'static str, OnceCell<Rc<CMap>>>,
     files: HashMap<Name, Rc<CMap>>
    }

impl Default for CMapRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl CMapRegistry {
    pub fn new() -> Self {
        Self {
            predefined: (PREDEFINED_CMAPS.keys().copied().map(|k| (k, OnceCell::new()))).collect(),
            files: HashMap::new(),
        }
    }

    pub fn add(&mut self, cmap: CMap) {
        self.files.insert(cmap.name.clone(), Rc::new(cmap));
    }

    pub fn get(&self, name: &Name) -> Option<Rc<CMap>> {
        self.predefined
            .get(name.as_str())
            .map(|c| Rc::clone(c.get_or_init(|| {
                let file = PREDEFINED_CMAPS.get(name.as_str()).unwrap();
                Rc::new(self.parse_cmap_file(file).unwrap())
            })))
            .or_else(|| self.files.get(name).cloned())
    }

    fn parse_cmap_file(&self, file: &[u8]) -> anyhow::Result<CMap> {
        let p = CMapMachinePlugin {
            registry: self,
            parsed: None,
            n_code_space: 0,
            code_space: None,
            cid_range_parsing: None,
            cid_range_entries: Default::default(),
            cid_char_parsing: None,
            cid_char_entries: Default::default(),
            bf_char_parsing: None,
            n_notdef_range: 0,
            notdef_range_entries: vec![],
            n_notdef_char: 0,
            notdef_char_entries: vec![],
            use_cmap: None,
        };
        let mut m = Machine::<CMapMachinePlugin>::with_plugin(file, p);
        m.execute()?;
        let mut p = m.take_plugin();
        Ok(p.parsed.take().expect("CMap not defined in cmap file"))
    }

    /// Add a CMap file, parse it and add to registry.
    pub fn add_cmap_file(&mut self, file: &[u8]) -> anyhow::Result<Rc<CMap>> {
        let parsed = self.parse_cmap_file(file)?;
        let name = parsed.name.clone();
        self.add(parsed);
        self.get(&name)
            .ok_or_else(|| anyhow::anyhow!("CMap not found: {:?}", name))
    }
}

/// CMap maps sequence CharCode to sequence of CIDs.
#[derive(Debug, PartialEq, Eq)]
pub struct CMap {
    pub cid_system_info: CIDSystemInfo,
    pub w_mode: WriteMode,
    pub name: Name,

    code_space: CodeSpace,
    cid_map: Mapper<IncRangeMap>,
    notdef_map: Mapper<RangeMapToOne>,
    use_map: Option<Rc<CMap>>,
}

const DEFAULT_NOTDEF: CID = CID(0);

impl CMap {
    /// Map(Decode) char codes to CIDs.
    /// If code out of code space, or not mapped to cid, use notdef_map to map to a designed notdef
    /// char, if code not in notdef_map, returns 0 (notdef).
    pub fn map(&self, mut codes: &[u8]) -> Vec<CID> {
        let mut r = Vec::with_capacity(codes.len());
        while !codes.is_empty() {
            let code;
            (codes, code) = self.next_cid(codes);
            let cid = code.map_left(|c| self.map_undef(c)).into_inner();

            r.push(cid);
        }
        r
    }

    /// Get next cid, update codes buffer, without map notdef.
    /// If use_map not null, recover codes buffer, call next_cid.
    fn next_cid<'a>(&self, codes: &'a [u8]) -> (&'a [u8], Either<CharCode, CID>) {
        let (new_codes, code) = self.code_space.next_code(codes);
        let cid_or_code = code.right_and_then(|c| self.cid_map.map(c).ok_or(c).into());

        let Some(use_map) = self.use_map.as_ref() else {
            return (new_codes, cid_or_code);
        };

        cid_or_code
            .map_either(|_| use_map.next_cid(codes), |cid| (new_codes, Right(cid)))
            .into_inner()
    }

    /// Map undef cid, if notdef_map failed, call use_map.map_undef() if has use_map
    fn map_undef(&self, ch: CharCode) -> CID {
        self.notdef_map.map(ch).unwrap_or_else(|| {
            self.use_map
                .as_ref()
                .map_or(DEFAULT_NOTDEF, |m| m.map_undef(ch))
        })
    }
}

trait EntryParse: Sized {
    fn parse_entry<P>(m: &mut Machine<P>) -> Result<Self, MachineError>; 
}

struct EntriesParsing<T> {
    n: usize,
    _phantom: PhantomData<T>,
}

impl<T: EntryParse> EntriesParsing<T> {
    fn new<P>(m: &mut Machine<P>) -> Result<Self, MachineError> {
Ok(Self {
                    n: m.pop()?.int()? as usize,
                    _phantom: PhantomData,
                })
    } 
    
    fn on_end<P>(self, m: &mut Machine<P>) -> Result<Vec<T>, MachineError> {
        let mut entries = Vec::with_capacity(self.n);
        for _ in 0..self.n {
            entries.push(T::parse_entry(m)?);
        }
        entries.reverse();
        Ok(entries)
    }
}

#[derive(Educe)]
#[educe(Default)]
struct EntriesParser<T> {
    entries: Vec<T>,
}

impl<T: EntryParse> EntriesParser<T> {
    fn extend(&mut self, entries: Vec<T>) {
        self.entries.extend(entries);
    }
    
    fn take(&mut self) -> Vec<T> {
        std::mem::replace(&mut self.entries, vec![])
    }
}

/// CMap Machine plugin.
struct CMapMachinePlugin<'a> {
    registry: &'a CMapRegistry,
    parsed: Option<CMap>,
    use_cmap: Option<Rc<CMap>>,
    n_code_space: usize,
    code_space: Option<CodeSpace>,

    cid_range_parsing: Option<EntriesParsing<IncRangeMap>>,
    cid_range_entries: EntriesParser<IncRangeMap>,

    cid_char_parsing: Option<EntriesParsing<SingleCodeMap>>,
    cid_char_entries: EntriesParser<SingleCodeMap>,

    bf_char_parsing: Option<EntriesParsing<SingleCodeMap>>,

    n_notdef_range: usize,
    notdef_range_entries: Vec<RangeMapToOne>,
    n_notdef_char: usize,
    notdef_char_entries: Vec<SingleCodeMap>,
}

macro_rules! built_in_ops {
    ($($k:literal => $v:expr),* $(,)?) => {
        std::iter::Iterator::collect(std::iter::IntoIterator::into_iter([$((Key::Name(Name::from_static($k)), RuntimeValue::<CMapMachinePlugin>::BuiltInOp($v)),)*]))
    };
}

impl<'a> MachinePlugin for CMapMachinePlugin<'a> {
    fn find_proc_set_resource<'b>(
        &self,
        name: &Name,
    ) -> Option<crate::machine::RuntimeDictionary<'b, Self>> {
        (name == "CIDInit").then(|| -> HashMap<Key, RuntimeValue<'_, Self>> {
            built_in_ops!(
                "begincmap" => |_| {
                    ok()
                },
                "endcmap" => |_| {
                    ok()
                },
                "CMapName" => |m| {
                    let d = m.current_dict();
                    m.push(d.borrow().get(&sname("CMapName")).unwrap().clone());
                    ok()
                },
                "begincodespacerange" => |m| {
                    // pop a int from stack, the code space range entries.
                    m.p.n_code_space = m.pop()?.int()? as usize;
                    ok()
                },
                "endcodespacerange" => |m| {
                    let mut entries = Vec::with_capacity(m.p.n_code_space);
                    for _ in 0..m.p.n_code_space {
                        let s_upper = m.pop()?.string()?;
                        let s_lower = m.pop()?.string()?;
                        entries.push(CodeRange::from_str_buf(
                            &s_lower.borrow(),
                            &s_upper.borrow(),
                        ).ok_or_else(
                            || {
                                error!("Invalid code space range");
                                MachineError::TypeCheck
                            }
                        ));
                    } 
                    m.p.code_space = Some(CodeSpace::new(entries.into_iter().rev().collect::<Result<_, _>>()?));
                    ok()
                },
                "begincidrange" => |m| {
                    m.p.cid_range_parsing = Some(EntriesParsing::new(m)?);
                    ok()
                },
                "endcidrange" => |m| {
                    let entries = m.p.cid_range_parsing.take().unwrap().on_end(m)?;
                    m.p.cid_range_entries.extend(entries);
                    ok()
                },
                "begincidchar" => |m| {
                    m.p.cid_char_parsing = Some(EntriesParsing::new(m)?);
                    ok()
                },
                "endcidchar" => |m| {
                    let entries = m.p.cid_char_parsing.take().unwrap().on_end(m)?;
                    m.p.cid_char_entries.extend(entries);
                    ok()
                },
                "beginbfchar" => |m| {
                    m.p.bf_char_parsing = Some(EntriesParsing::new(m)?);
                    ok()
                },
                "endbfchar" => |m| {
                    let entries = m.p.bf_char_parsing.take().unwrap().on_end(m)?;
                    m.p.cid_char_entries.extend(entries);
                    ok()
                },
                "beginnotdefrange" => |m| {
                    m.p.n_notdef_range = m.pop()?.int()? as usize;
                    ok()
                },
                "endnotdefrange" => |m| {
                    let mut entries = Vec::with_capacity(m.p.n_notdef_range);
                    for _ in 0..m.p.n_notdef_range {
                        let cid = m.pop()?.int()?.try_into().unwrap();
                        let s_upper = m.pop()?.string()?;
                        let s_lower = m.pop()?.string()?;
                        entries.push(RangeMapToOne {
                            range: CodeRange::from_str_buf(
                                &s_lower.borrow(),
                                &s_upper.borrow(),
                            ).ok_or_else(
                                || {
                                    error!("Invalid notdef range");
                                    MachineError::TypeCheck
                                }
                            )?,
                            cid: CID(cid),
                        });
                    }
                    m.p.notdef_range_entries.extend(entries.into_iter().rev());
                    ok()
                },
                "beginnotdefchar" => |m| {
                    m.p.n_notdef_char = m.pop()?.int()? as usize;
                    ok()
                },
                "endnotdefchar" => |m| {
                    let mut entries = Vec::with_capacity(m.p.n_notdef_char);
                    for _ in 0..m.p.n_notdef_char {
                        let cid = m.pop()?.int()?.try_into().unwrap();
                        let s_code = m.pop()?.string()?;
                        entries.push(SingleCodeMap {
                            code: CharCode::from_str_buf(&s_code.borrow()),
                            cid: CID(cid),
                        });
                    }
                    m.p.notdef_char_entries.extend(entries.into_iter().rev());
                    ok()
                },
                "defineresource" => |m| {
                    let res_category = m.pop()?.name()?;
                    assert_eq!(res_category, sname("CMap"));
                    let d = m.pop()?.dict()?;
                    let d_ref = d.borrow();
                    let cmap_name = m.pop()?.name()?;
                    let cmap = CMap {
                        cid_system_info: CIDSystemInfo::from_dict(&d_ref[&sname("CIDSystemInfo")].dict()?.borrow())?,
                        w_mode: WriteMode::parse(d_ref[&sname("WMode")].int()?)?,
                        name: cmap_name,
                        code_space: m.p.code_space.take().unwrap_or_default(),
                        cid_map: Mapper {
                            ranges: m.p.cid_range_entries.take().into(),
                            chars: m.p.cid_char_entries.take().into(),
                        },
                        notdef_map: Mapper{
                            ranges: m.p.notdef_range_entries.drain(..).collect(),
                            chars: m.p.notdef_char_entries.drain(..).collect(),
                        },
                        use_map: m.p.use_cmap.take(),
                    };
                    m.p.parsed = Some(cmap);
                    // should push cmap object to stack, but cmap object not RuntimeValue
                    // so push a dummy value. This value normally is not used and pop up immediately.
                    m.push(sname("cmap stub"));
                    ok()
                },
                "usecmap" => |m| {
                    let name = m.pop()?.name()?;
                    let cmap = m.p.registry.get(&name).ok_or_else(|| {
                        error!("CMap not found: {:?}", name);
                        MachineError::Undefined
                    })?;
                    m.p.use_cmap = Some(cmap);
                    ok()
                },
            )
        })
    }
}

#[cfg(test)]
mod tests;
