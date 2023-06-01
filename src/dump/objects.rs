use crate::dump::object::ObjectIdDumper;
use std::fmt::Display;

use super::object::ObjectDumper;
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

/// Dump objects in the `document`, if `id` is `None`, dump all objects, otherwise dump the object with `id`
pub fn dump_objects(doc: &Document, id: Option<u32>) {
    let objects = &doc.objects;
    let mut not_found = true;
    objects
        .iter()
        .filter(|(obj_id, _)| equals_to_id(id, obj_id))
        .for_each(|(id, obj)| {
            not_found = false;
            println!("{}", ObjectEntryDumper(id, obj));
        });
    if not_found {
        println!("Object not found");
    }
}

#[cfg(test)]
mod tests;
