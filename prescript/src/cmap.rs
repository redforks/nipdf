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
use std::{collections::HashMap, rc::Rc, str::from_utf8};
use tinyvec::ArrayVec;

/// Convert from CharCode using cmap, use it to select glyph id
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CID(u16);

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

    /// Parse a range from string, for example:
    ///
    /// `parse("00", "08")`, returns a range that matches 1 byte, from 0x00 to 0x08.
    /// `parse("0000", "0800")`, returns a range that matches 2 bytes, from 0x0000 to 0x0800.
    fn parse(s_lower: &str, s_upper: &str) -> Option<Self> {
        let lower = u32::from_str_radix(s_lower, 16).ok()?;
        let upper = u32::from_str_radix(s_upper, 16).ok()?;
        let n_bytes = s_lower.len() / 2;
        assert_eq!(n_bytes, s_upper.len() / 2);
        let mut r = ArrayVec::new();
        for i in (0..n_bytes).into_iter().rev() {
            let lower = (lower >> (i * 8)) as u8;
            let upper = (upper >> (i * 8)) as u8;
            r.push(ByteRange::new(lower, upper));
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
        for (r, c) in self.0.iter().zip(ch.as_ref().into_iter().copied()) {
            if !r.in_range(c) {
                return None;
            }
            offset = offset * (r.upper - r.lower + 1) as u16 + (c - r.lower) as u16;
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

/// Maps a range of codes to CID, all codes in range map to `cid`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RangeMapToOne {
    range: CodeRange,
    cid: CID,
}

impl CodeMap for RangeMapToOne {
    fn map(&self, code: CharCode) -> Option<CID> {
        (self.range.in_range(code)).then(|| self.cid)
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
        (code == self.code).then(|| self.cid)
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

/// CMapRegistry contains all CMaps, access by CMap Name.
#[derive(Debug, Clone, PartialEq, Educe)]
#[educe(Default(new))]
pub struct CMapRegistry(HashMap<Name, Rc<CMap>>);

impl CMapRegistry {
    pub fn add(&mut self, cmap: CMap) {
        self.0.insert(cmap.name.clone(), Rc::new(cmap));
    }

    pub fn get(&self, name: &Name) -> Option<Rc<CMap>> {
        self.0.get(name).cloned()
    }

    /// Add a CMap file, parse it and add to registry.
    pub fn add_cmap_file(&mut self, file: &[u8]) -> anyhow::Result<Rc<CMap>> {
        let p = CMapMachinePlugin {
            registry: self,
            parsed: None,
            n_code_space: 0,
            code_space: None,
            n_cid_range: 0,
            cid_range_entries: vec![],
            n_cid_char: 0,
            cid_char_entries: vec![],
        };
        let mut m = Machine::<CMapMachinePlugin>::with_plugin(file, p);
        m.execute()?;
        let mut p = m.take_plugin();
        let parsed = p.parsed.take().expect("CMap not defined in cmap file");
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

/// CMap Machine plugin.
struct CMapMachinePlugin<'a> {
    registry: &'a CMapRegistry,
    parsed: Option<CMap>,
    n_code_space: usize,
    code_space: Option<CodeSpace>,
    n_cid_range: usize,
    cid_range_entries: Vec<IncRangeMap>,
    n_cid_char: usize,
    cid_char_entries: Vec<SingleCodeMap>,
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
                    error!("TODO: begincmap");
                    ok()
                },
                "endcmap" => |_| {
                    error!("TODO: endcmap");
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
                    m.p.n_cid_range = m.pop()?.int()? as usize;
                    ok()
                },
                "endcidrange" => |m| {
                    let mut entries = Vec::with_capacity(m.p.n_cid_range);   
                    for _ in 0..m.p.n_cid_range {
                        let cid = m.pop()?.int()? as u16;
                        let s_upper = m.pop()?.string()?;
                        let s_lower = m.pop()?.string()?;
                        entries.push(IncRangeMap {
                            range: CodeRange::from_str_buf(
                                &s_lower.borrow(),
                                &s_upper.borrow(),
                            ).ok_or_else(
                                || {
                                    error!("Invalid cid range");
                                    MachineError::TypeCheck
                                }
                            )?,
                            start_cid: CID(cid),
                        });
                    }
                    m.p.cid_range_entries.extend(entries.into_iter().rev());
                    ok()
                },
                "begincidchar" => |m| {
                    m.p.n_cid_char = m.pop()?.int()? as usize; 
                    ok()
                },
                "endcidchar" => |m| {
                    let mut entries = Vec::with_capacity(m.p.n_cid_char);
                    for _ in 0..m.p.n_cid_char {
                        let cid = m.pop()?.int()? as u16;
                        let s_code = m.pop()?.string()?;
                        entries.push(SingleCodeMap {
                            code: CharCode::from_str_buf(&s_code.borrow()),
                            cid: CID(cid),
                        });
                    }
                    m.p.cid_char_entries.extend(entries.into_iter().rev());
                    ok()
                },
                "beginnotdefrange" => |_| {
                    error!("TODO: beginnotdefrange");
                    ok()
                },
                "endnotdefrange" => |_| {
                    error!("TODO: endnotdefrange");
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
                            ranges: m.p.cid_range_entries.drain(..).collect(),
                            chars: m.p.cid_char_entries.drain(..).collect(),
                        } ,
                        notdef_map: Mapper::default(),
                        use_map: None,
                    };
                    m.p.parsed = Some(cmap);
                    // should push cmap object to stack, but cmap object not RuntimeValue
                    // so push a dummy value. This value normally is not used and pop up immediately.
                    m.push(sname("cmap stub"));
                    ok()
                },
            )
        })
    }
}

#[cfg(test)]
mod tests;
