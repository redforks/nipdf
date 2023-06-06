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

/// Dump objects in the `document`, if `id` is `None`, dump all objects, otherwise dump the object with `id`
pub fn dump_objects(f: &FileWithXRef, id: Option<u32>, dump_content: bool, decode: bool) {
    for (id, obj) in f.iter_id_object() {
        println!(
            "{}: {}",
            PrimitiveDumper::new(&id.into()),
            PrimitiveDumper::new(&obj)
        );
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
