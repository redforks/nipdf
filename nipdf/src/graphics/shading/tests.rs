use std::num::NonZeroU32;
use test_case::test_case;

use crate::file::{ObjectResolver, XRefTable};

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

#[test_case(b"1 0 obj<</ShadingType 3/ColorSpace/DeviceGray/Coords[1 1 0 1 1 0]>>endobj"; "radius both be zero")]
#[test_case(b"1 0 obj<</ShadingType 3/ColorSpace/DeviceGray/Coords[1 1 -1 1 1 1]>>endobj"; "negative start radius")]
#[test_case(b"1 0 obj<</ShadingType 3/ColorSpace/DeviceGray/Coords[1 1 1 1 1 -1]>>endobj"; "negative end radius")]
#[test_case(b"1 0 obj<</ShadingType 3/ColorSpace/DeviceGray/Coords[1 1 2 1 1 1]>>endobj"; "end radius less than start radius")]
fn build_invalid_radial(buf: &[u8]) -> AnyResult<()> {
    let xref = XRefTable::from_buf(buf);
    let resolver = ObjectResolver::new(buf, &xref);
    let d: RadialShadingDict = resolver.resolve_pdf_object(NonZeroU32::new(1).unwrap())?;
    assert_eq!(None, build_radial(&d)?);
    Ok(())
}
