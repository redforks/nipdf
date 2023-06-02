use crate::dump::object::ObjectIdDumper;
use std::{fmt::Display, io::stdout};

use super::object::{ObjectDumper, StreamDumper};
use lopdf::{Document, Object, ObjectId};

struct ObjectEntryDumper<'a>(&'a ObjectId, &'a Object);

impl<'a> Display for ObjectEntryDumper<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "{}: {}",
            ObjectIdDumper::new(self.0),
            ObjectDumper::new(self.1)
        ))
    }
}

/// Returns true if `obj_id` id equals to `id`, if `id` is None, return true.
fn equals_to_id(id: Option<u32>, obj_id: &ObjectId) -> bool {
    id.map_or(true, |id| obj_id.0 == id)
}

fn filter_by_id(doc: &Document, id: Option<u32>) -> impl Iterator<Item = (&ObjectId, &Object)> {
    doc.objects
        .iter()
        .filter(move |(obj_id, _)| equals_to_id(id, obj_id))
}

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

fn dump_stream_content(doc: &Document, id: Option<u32>) {
    match exactly_one(filter_by_id(doc, id)) {
        Ok((_, obj)) => match obj {
            Object::Stream(stream) => {
                StreamDumper::new(stream).write_content(stdout()).unwrap();
            }
            _ => {
                eprintln!("Object not stearm");
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
}

/// Dump objects in the `document`, if `id` is `None`, dump all objects, otherwise dump the object with `id`
pub fn dump_objects(doc: &Document, id: Option<u32>, dump_content: bool, decode: bool) {
    if dump_content {
        dump_stream_content(doc, id)
    } else {
        let mut not_found = true;
        filter_by_id(doc, id).for_each(|(id, obj)| {
            not_found = false;
            println!("{}", ObjectEntryDumper(id, obj));
        });
        if not_found {
            println!("Object not found");
        }
    }
}

#[cfg(test)]
mod tests;
