use std::fmt::Display;

use lopdf::xref::Xref;

pub struct XrefDumper<'a>(&'a Xref);

impl<'a> XrefDumper<'a> {
    pub fn new(xref: &'a Xref) -> Self {
        Self(xref)
    }
}

impl<'a> Display for XrefDumper<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("xref:\n")?;
        f.write_fmt(format_args!("type: {:?}\n", self.0.cross_reference_type))?;
        f.write_fmt(format_args!("size: {}\n", self.0.size))
    }
}
