use super::*;
use std::fmt::Debug;
use test_case::test_case;

#[test_case(true, true)]
#[test_case(Object::Null, None::<bool>)]
fn test_de<'a, T: PartialEq + Debug + for<'b> Deserialize<'b>>(o: impl Into<Object<'a>>, exp: T) {
    let o = o.into();
    assert_eq!(from_object::<T>(&o).unwrap(), exp);
}
