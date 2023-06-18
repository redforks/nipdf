use super::Dictionary;

#[derive(Clone, PartialEq, Debug)]
pub struct Stream<'a>(pub Dictionary<'a>, pub &'a [u8]);
