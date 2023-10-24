use super::*;
use euclid::approxeq::ApproxEq;
use euclid::default::Transform2D as Transform;
use euclid::Point2D;

#[test]
fn to_skia() {
    let m = Transform::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
    let skia = m.into_skia();
    assert_eq!(skia.sx, 1.0);
    assert_eq!(skia.ky, 2.0);
    assert_eq!(skia.kx, 3.0);
    assert_eq!(skia.sy, 4.0);
    assert_eq!(skia.tx, 5.0);
    assert_eq!(skia.ty, 6.0);
}

fn new_assert<S, T, SP: Into<Point2D<f32, S>>, TP: Into<Point2D<f32, T>>>(
    m: Transform2D<f32, S, T>,
) -> impl Fn(SP, TP) {
    move |p, exp| {
        let exp = exp.into();
        let p = m.transform_point(p.into());
        assert!(p.approx_eq(&exp), "exp != actual: {:?} != {:?}", &exp, p);
    }
}

#[test]
fn test_user_to_device_space() {
    // ctm is identity, no zoom, flip y
    let f = new_assert(user_to_device_space(600.0, 1.0, Transform2D::identity()));
    f((0.0, 0.0), (0.0, 600.0));
    f((10.0, 20.0), (10.0, 600.0 - 20.0));

    // ctm is identity, zoom 1.5, flip y
    let f = new_assert(user_to_device_space(600.0, 1.5, Transform2D::identity()));
    f((0.0, 0.0), (0.0, 600.0 * 1.5));
    f((10.0, 20.0), (10.0 * 1.5, 600.0 * 1.5 - 20.0 * 1.5));

    // ctm contains transform, zoom 1.5, flip y
    let f = new_assert(user_to_device_space(
        600.0,
        1.5,
        Transform2D::translation(10.0, 20.0),
    ));
    f((0.0, 0.0), (10.0 * 1.5, (600.0 - 20.) * 1.5));
    f((10.0, 20.0), (20.0 * 1.5, (600.0 - 40.) * 1.5));

    // ctm contains scale and transform, zoom 1.5, flip y
    assert_eq!(
        Transform::scale(2.0, 3.0).then_translate((10.0, 20.0).into()),
        Transform::new(2.0, 0.0, 0.0, 3.0, 10.0, 20.0),
        "scale then translate, translates not scaled"
    );
    let f = new_assert(user_to_device_space(
        600.0,
        1.5,
        Transform2D::scale(2.0, 3.0).then_translate((10.0, 20.0).into()),
    ));
    f((0.0, 0.0), (10.0 * 1.5, (600.0 - 20.) * 1.5));
    f(
        (11.0, 15.0),
        // x: scale 2., then move 10. (because move not scaled)
        ((11.0 * 2. + 10.0) * 1.5, (600.0 - (15. * 3. + 20.)) * 1.5),
    );
}
