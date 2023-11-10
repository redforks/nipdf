use std::num::NonZeroU32;
use test_case::test_case;

use crate::{
    file::{ObjectResolver, XRefTable},
    object::Dictionary,
};

use super::*;

#[test]
fn radial_coords_try_from() {
    let o = Object::Array(vec![
        1.into(),
        2_f32.into(),
        3.into(),
        4.into(),
        5.into(),
        6.into(),
    ]);
    let coords = RadialCoords::try_from(&o).unwrap();
    assert_eq!(
        coords,
        RadialCoords {
            start: RadialCircle {
                point: Point { x: 1., y: 2. },
                r: 3.
            },
            end: RadialCircle {
                point: Point { x: 4., y: 5. },
                r: 6.
            },
        }
    );
}

#[test]
fn axias_coords_try_from() {
    let o = Object::Array(vec![1.into(), 2_f32.into(), 3.into(), 4.into()]);
    let coords = AxialCoords::try_from(&o).unwrap();
    assert_eq!(
        coords,
        AxialCoords {
            start: Point { x: 1., y: 2. },
            end: Point { x: 3., y: 4. },
        }
    );
}

#[test_case(b"1 0 obj<</ShadingType 3/ColorSpace/DeviceGray/Coords[1 1 0 1 1 0]>>endobj"; "radius both be zero")]
#[test_case(b"1 0 obj<</ShadingType 3/ColorSpace/DeviceGray/Coords[1 1 -1 1 1 1]>>endobj"; "negative start radius")]
#[test_case(b"1 0 obj<</ShadingType 3/ColorSpace/DeviceGray/Coords[1 1 1 1 1 -1]>>endobj"; "negative end radius")]
fn build_invalid_radial(buf: &[u8]) -> AnyResult<()> {
    let xref = XRefTable::from_buf(buf);
    let resolver = ObjectResolver::new(buf, &xref, None);
    let d: ShadingDict = resolver.resolve_pdf_object(NonZeroU32::new(1).unwrap())?;
    let empty_d = Dictionary::new();
    let resource = ResourceDict::new(None, &empty_d, &resolver)?;
    assert_eq!(None, build_radial(&d, &resource)?);
    Ok(())
}
