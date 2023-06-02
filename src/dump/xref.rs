use std::fmt::Display;

use lopdf::{xref::XrefEntry, Document};

struct XrefEntryDumper<'a>(u32, &'a XrefEntry);

impl<'a> Display for XrefEntryDumper<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{}: ", self.0))?;

        match self.1 {
            XrefEntry::Free => f.write_str("free"),
            XrefEntry::UnusableFree => f.write_str("unusable free"),
            XrefEntry::Normal { offset, generation } => {
                f.write_fmt(format_args!("normal {} {}", offset, generation))
            }
            XrefEntry::Compressed { container, index } => {
                f.write_fmt(format_args!("compressed {} {}", container, index))
            }
        }
    }
}

pub fn dump_xref(doc: &Document, id: Option<u32>) {
    let xref = &doc.reference_table;
    match id {
        None => {
            xref.entries.iter().for_each(|(id, entry)| {
                println!("{}", XrefEntryDumper(*id, entry));
            });
        }
        Some(id) => match xref.get(id) {
            None => println!("{}: not found", id),
            Some(entry) => println!("{}", XrefEntryDumper(id, entry)),
        },
    }
}

#[cfg(test)]
mod tests;
