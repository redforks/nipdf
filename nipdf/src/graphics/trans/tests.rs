#![allow(clippy::suboptimal_flops)]

use super::*;
use euclid::{approxeq::ApproxEq, default::Transform2D as Transform, Angle, Point2D};

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
        assert!(
            p.approx_eq_eps(&exp, &(0.0001, 0.0001).into()),
            "exp != actual: {:?} != {:?}",
            &exp,
            p
        );
    }
}

fn to_device_space<S>(
    logic_device_height: impl AsPrimitive<f32>,
    zoom: f32,
    to_logic_device: &Transform2D<f32, S, LogicDeviceSpace>,
) -> Transform2D<f32, S, DeviceSpace> {
    to_logic_device.then(&logic_device_to_device(logic_device_height, zoom))
}

#[test]
fn test_user_to_device_space() {
    // ctm is identity, no zoom, flip y
    let f = new_assert(to_device_space::<UserSpace>(
        600.0,
        1.0,
        &Transform2D::identity(),
    ));
    f((0.0, 0.0), (0.0, 600.0));
    f((10.0, 20.0), (10.0, 600.0 - 20.0));

    // ctm is identity, zoom 1.5, flip y
    let f = new_assert(to_device_space::<UserSpace>(
        600.0,
        1.5,
        &Transform2D::identity(),
    ));
    f((0.0, 0.0), (0.0, 600.0 * 1.5));
    f((10.0, 20.0), (10.0 * 1.5, 600.0 * 1.5 - 20.0 * 1.5));

    // ctm contains transform, zoom 1.5, flip y
    let f = new_assert(to_device_space::<UserSpace>(
        600.0,
        1.5,
        &Transform2D::translation(10.0, 20.0),
    ));
    f((0.0, 0.0), (10.0 * 1.5, (600.0 - 20.) * 1.5));
    f((10.0, 20.0), (20.0 * 1.5, (600.0 - 40.) * 1.5));

    // ctm contains scale and transform, zoom 1.5, flip y
    assert_eq!(
        Transform::scale(2.0, 3.0).then_translate((10.0, 20.0).into()),
        Transform::new(2.0, 0.0, 0.0, 3.0, 10.0, 20.0),
        "scale then translate, translates not scaled"
    );
    let f = new_assert(to_device_space::<UserSpace>(
        600.0,
        1.5,
        &Transform2D::scale(2.0, 3.0).then_translate((10.0, 20.0).into()),
    ));
    f((0.0, 0.0), (10.0 * 1.5, (600.0 - 20.) * 1.5));
    f(
        (11.0, 15.0),
        // x: scale 2., then move 10. (because move not scaled)
        ((11.0 * 2. + 10.0) * 1.5, (600.0 - (15. * 3. + 20.)) * 1.5),
    );
}

#[test]
fn test_image_space_to_user_space() {
    let f = new_assert(image_to_user_space(100, 200));
    f((0.0, 0.0), (0.0, 1.0));
    f((40.0, 80.0), (0.4, 0.6));
    f((0., 200.), (0., 0.));
    f((100., 0.), (1., 1.));
    f((100., 200.), (1., 0.));
}

fn image_to_device_space(
    img_w: u32,
    img_h: u32,
    logic_device_height: impl AsPrimitive<f32>,
    zoom: f32,
    ctm: &UserToLogicDeviceSpace,
) -> ImageToDeviceSpace {
    image_to_user_space(img_w, img_h)
        .then(ctm)
        .then(&logic_device_to_device(logic_device_height, zoom))
}

#[test]
fn test_image_to_device_space() {
    let f = new_assert(image_to_device_space(
        1107,
        1352,
        648.,
        1.,
        &UserToLogicDeviceSpace::new(531.0, 0.0, 0.0, 648.0, 0.0, 0.0),
    ));
    f((0., 0.), (0., 0.));
    f((1107., 0.), (531., 0.));
    f((1107., 1352.), (531., 648.));

    let r = image_to_device_space(
        512,
        512,
        842.,
        1.5,
        &UserToLogicDeviceSpace::new(383.9, 0.0, 0.0, 383.9, 105.7, 401.5),
    );
    let f = new_assert(r);
    f((0., 0.), (105.7 * 1.5, (842. - (401.5 + 383.9)) * 1.5));
}

#[test]
fn test_move_text_space_right() {
    let f = new_assert(move_text_space_right(
        &Transform2D::identity()
            .then_scale(2.0, 3.)
            .then_rotate(Angle::degrees(90.)),
        10.0,
    ));
    f((0., 0.), (0. * 3., 10. * 2.));
}
