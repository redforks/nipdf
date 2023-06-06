use std::{borrow::Borrow, sync::Arc};

use pdf::{
    any::AnySync,
    file::{Cache, File, FileOptions, NoCache},
    object::{NoResolve, ObjNr, PlainRef, Resolve, Stream},
    primitive::Primitive,
    xref::XRefTable,
    PdfError,
};

use super::dump_primitive::PrimitiveDumper;
use super::FileWithXRef;

fn equals_to_id(id: Option<u32>, obj_id: &PlainRef) -> bool {
    id.map_or(true, |id| id as ObjNr == obj_id.id)
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ExactlyOneError {
    NoItems,
    MoreThanOne,
}

fn exactly_one<T>(mut iter: impl Iterator<Item = T>) -> Result<T, ExactlyOneError> {
    match iter.next() {
        Some(item) => match iter.next() {
            Some(_) => Err(ExactlyOneError::MoreThanOne),
            None => Ok(item),
        },
        None => Err(ExactlyOneError::NoItems),
    }
}

/// Dump objects in the `document`, if `id` is `None`, dump all objects, otherwise dump the object with `id`
pub fn dump_objects(f: &FileWithXRef, id: Option<u32>, dump_content: bool) {
    let mut iter = f.iter_id_object().filter(|(r, _)| equals_to_id(id, &r));
    if dump_content {
        match exactly_one(iter) {
            Ok((_, obj)) => match obj {
                Primitive::Stream(pdf_stream) => {
                    let stream: Stream<()> =
                        Stream::from_stream(pdf_stream.clone(), f.f()).unwrap();
                    let data = stream.data(f.f()).unwrap();
                    let mut data: &[u8] = data.borrow();
                    std::io::copy(&mut data, &mut std::io::stdout()).unwrap();
                }
                _ => {
                    eprintln!("Object not stream");
                }
            },
            Err(err) => match err {
                ExactlyOneError::NoItems => {
                    eprintln!("Object not found");
                }
                ExactlyOneError::MoreThanOne => {
                    eprintln!("More than one object found");
                }
            },
        }
        return;
    }

    let mut not_found = true;
    for (id, obj) in iter {
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
}

#[cfg(test)]
mod tests;
