use super::*;




#[test]
fn radial_coords_try_from() {
    let o = vec![
        1.into(),
        2_f32.into(),
        3.into(),
        4.into(),
        5.into(),
        6.into(),
    ]
    .into();
    let coords = RadialCoords::try_from(&o).unwrap();
    assert_eq!(
        coords,
        RadialCoords {
            start: RadialCircle {
                point: Point::new(1., 2.),
                r: 3.
            },
            end: RadialCircle {
                point: Point::new(4., 5.),
                r: 6.
            },
        }
    );
}

#[test]
fn axias_coords_try_from() {
    let o = vec![1.into(), 2_f32.into(), 3.into(), 4.into()].into();
    let coords = AxialCoords::try_from(&o).unwrap();
    assert_eq!(
        coords,
        AxialCoords {
            start: Point::new(1., 2.),
            end: Point::new(3., 4.),
        }
    );
}
