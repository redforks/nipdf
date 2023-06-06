use std::sync::Arc;

use pdf::{
    any::AnySync,
    file::{Cache, File, FileOptions, NoCache},
    object::{NoResolve, ObjNr, PlainRef, Resolve},
    xref::XRefTable,
    PdfError,
};

use super::dump_primitive::PrimitiveDumper;
use super::FileWithXRef;

/// Dump objects in the `document`, if `id` is `None`, dump all objects, otherwise dump the object with `id`
pub fn dump_objects(f: &FileWithXRef, id: Option<u32>, dump_content: bool, decode: bool) {
    let table = f.xref_table();
    for id in table.iter() {
        if let Ok(xref) = table.get(id as ObjNr) {
            print!("{}: ", id);
            let plain_ref = PlainRef {
                id: id as ObjNr,
                gen: xref.get_gen_nr(),
            };
            if let Ok(obj) = f.f().resolve(plain_ref) {
                println!(
                    "{}: {}",
                    PrimitiveDumper::new(&plain_ref.into()),
                    PrimitiveDumper::new(&obj)
                );
            }
        }
    }
    // if dump_content {
    //     dump_stream_content(doc, id, decode)
    // } else {
    //     let mut not_found = true;
    //     filter_by_id(doc, id).for_each(|(id, obj)| {
    //         not_found = false;
    //         println!("{}", ObjectEntryDumper(id, obj));
    //     });
    //     if not_found {
    //         println!("Object not found");
    //     }
    // }
}
