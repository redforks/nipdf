use std::borrow::Cow;

use super::{Dictionary, ObjectValueError};

#[derive(Clone, PartialEq, Debug)]
pub struct Stream<'a>(pub Dictionary<'a>, pub &'a [u8]);

impl<'a> Stream<'a> {
    /// Decode stream data using filter and parameters in stream dictionary.
    pub fn decode(&self) -> Result<Cow<[u8]>, ObjectValueError> {
        todo!()
    }
}
