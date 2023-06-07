use pdf::{
    backend::Backend,
    file::{File, FileOptions, NoCache},
    object::{NoResolve, ObjNr, PlainRef, Resolve},
    primitive::Primitive,
    xref::XRefTable,
};
use std::fmt::{Display, Write};

pub mod dump_primitive;
pub mod objects;
pub mod page;
pub mod query;

#[derive(Clone, Copy, PartialEq, Debug)]
/// When display, render n*2 spaces
struct Indent(usize);

impl Indent {
    fn inc(self) -> Self {
        Self(self.0 + 1)
    }
}

impl Display for Indent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for _ in 0..(self.0 * 2) {
            f.write_char(' ')?;
        }
        Ok(())
    }
}

pub struct FileWithXRef {
    f: File<Vec<u8>, NoCache, NoCache>,
    xref_table: XRefTable,
}

impl FileWithXRef {
    pub fn open(p: &str) -> Self {
        let f = FileOptions::uncached().open(p).unwrap();
        let content: Vec<u8> = std::fs::read(p).unwrap();
        let start = content.locate_start_offset().unwrap();
        let (table, _) = content
            .read_xref_table_and_trailer(start, &NoResolve)
            .unwrap();
        Self {
            f,
            xref_table: table,
        }
    }

    pub fn f(&self) -> &File<Vec<u8>, NoCache, NoCache> {
        &self.f
    }

    pub fn xref_table(&self) -> &XRefTable {
        &self.xref_table
    }

    pub fn iter_id_object(&self) -> impl Iterator<Item = (PlainRef, Primitive)> + '_ {
        let table = self.xref_table();
        table.iter().filter_map(move |id| {
            if let Ok(xref) = table.get(id as ObjNr) {
                let plain_ref = PlainRef {
                    id: id as ObjNr,
                    gen: xref.get_gen_nr(),
                };
                if let Ok(obj) = self.f.resolve(plain_ref) {
                    Some((plain_ref, obj))
                } else {
                    None
                }
            } else {
                None
            }
        })
    }
}

#[cfg(test)]
mod tests;
