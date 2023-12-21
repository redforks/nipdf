//! Cmap to map CharCode to CID, used in Type0/CID font

/// Convert from CharCode using cmap, use it to select glyph id
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CID(u16);

/// Input code type, can be one/two/three bytes.
/// TODO: bytes in any length
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum CharCode {
    One(u8),
    Two(u8, u8),
    Three(u8, u8, u8),
}

/// Common trait that maps CharCode to CID.
trait CodeMap {
    fn map(&self, code: CharCode) -> Option<CID>;
}

/// A range entry in code space, lower and upper must have the same length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct CodeRange(CharCode, CharCode);

/// CodeSpace made up by CodeRanges.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CodeSpace(Box<[CodeRange]>);

impl CodeSpace {
    fn new(mut ranges: Vec<CodeRange>) -> Self {
        ranges.sort();
        Self(ranges.into_boxed_slice())
    }

    /// Take next code from input codes, return the rest codes and the next code.
    /// If next code not in code space, return `Err(next_code)`.
    /// Panic if input codes is empty.
    fn next_code<'a>(&self, codes: &'a [u8]) -> (&'a [u8], Result<CharCode, CharCode>) {
        todo!()
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
        todo!()
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
        todo!()
    }
}

/// Maps a single code to CID.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SingleCodeMap {
    code: CharCode,
    cid: CID,
}

impl CodeMap for SingleCodeMap {
    fn map(&self, code: CharCode) -> Option<CID> {
        todo!()
    }
}

/// Compound mapper that combines range and single code maps.
/// Single Code maps has higher priority than range maps.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Mapper<R> {
    ranges: Box<[R]>,
    chars: Box<[SingleCodeMap]>,
}

impl<R> CodeMap for Mapper<R> {
    fn map(&self, code: CharCode) -> Option<CID> {
        todo!()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CIDSystemInfo {
    registry: String,
    ordering: String,
    supplement: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    Horizontal = 0,
    Vertical = 1,
}

/// CMap maps sequence CharCode to sequence of CIDs.
#[derive(Debug, PartialEq, Eq)]
pub struct CMap {
    pub cid_system_info: CIDSystemInfo,
    pub w_mode: WriteMode,

    code_space: CodeSpace,
    cid_map: Mapper<IncRangeMap>,
    notdef_map: Mapper<RangeMapToOne>,
}

const DEFAULT_NOTDEF: CID = CID(0);

impl CMap {
    /// Map(Decode) char codes to CIDs.
    /// If code out of code space, returns 0 (notdef).
    /// If code not mapped, use `notdef_map` to map to a designed notdef char,
    /// if code not in notdef_map, returns 0 (notdef).
    pub fn map(&self, mut codes: &[u8]) -> Vec<CID> {
        let mut r = Vec::with_capacity(codes.len());
        while !codes.is_empty() {
            let code;
            (codes, code) = self.code_space.next_code(codes);
            let Ok(code) = code else {
                r.push(DEFAULT_NOTDEF);
                continue;
            };

            if let Some(cid) = self.cid_map.map(code) {
                r.push(cid);
                continue;
            }

            if let Some(notdef) = self.notdef_map.map(code) {
                r.push(notdef);
                continue;
            }

            r.push(DEFAULT_NOTDEF);
        }
        r
    }
}

#[cfg(test)]
mod tests;
