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
            start: RadialCircle { x: 1., y: 2., r: 3. },
            end: RadialCircle { x: 4., y: 5., r: 6. },
        }
    );
}
