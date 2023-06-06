use std::sync::Arc;

use pdf::{
    any::AnySync,
    file::{Cache, File, FileOptions, NoCache},
    object::NoResolve,
    xref::XRefTable,
    PdfError,
};

/// Dump objects in the `document`, if `id` is `None`, dump all objects, otherwise dump the object with `id`
pub fn dump_objects<OC, SC>(
    f: &File<Vec<u8>, OC, SC>,
    file_content: &[u8],
    id: Option<u32>,
    dump_content: bool,
    decode: bool,
) where
    OC: Cache<Result<AnySync, Arc<PdfError>>>,
    SC: Cache<Result<Arc<[u8]>, Arc<PdfError>>>,
{
    todo!();
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
