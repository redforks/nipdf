use super::{Dictionary, XRefSection};

#[derive(Debug, Clone, PartialEq)]
/// Frame contains things like xref, trailer, caused by incremental update. See [FrameSet]
pub struct Frame<'a> {
    pub xref_pos: u32,
    pub trailer: Dictionary<'a>,
    pub xref_section: XRefSection,
}

impl<'a> Frame<'a> {
    pub fn new(xref_pos: u32, trailer: Dictionary<'a>, xref_section: XRefSection) -> Self {
        Self {
            xref_pos,
            trailer,
            xref_section,
        }
    }
}

pub type FrameSet<'a> = Vec<Frame<'a>>;
