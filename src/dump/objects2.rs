use std::sync::Arc;

use pdf::{
    any::AnySync,
    file::{Cache, File, FileOptions, NoCache},
    object::{NoResolve, ObjNr, PlainRef, Resolve},
    primitive::Primitive,
    xref::XRefTable,
    PdfError,
};

use super::dump_primitive::PrimitiveDumper;
use super::FileWithXRef;

fn equals_to_id(id: Option<u32>, obj_id: &PlainRef) -> bool {
    id.map_or(true, |id| id as ObjNr == obj_id.id)
}

/// Dump objects in the `document`, if `id` is `None`, dump all objects, otherwise dump the object with `id`
pub fn dump_objects(f: &FileWithXRef, id: Option<u32>, dump_content: bool, decode: bool) {
    let mut not_found = true;
    for (id, obj) in f.iter_id_object().filter(|(r, _)| equals_to_id(id, &r)) {
        not_found = false;
        println!(
            "{}: {}",
            PrimitiveDumper::new(&id.into()),
            PrimitiveDumper::new(&obj)
        );
    }
    if not_found {
        println!("Object not found");
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
