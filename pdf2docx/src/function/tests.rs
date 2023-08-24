use super::*;
use crate::object::PdfObject;
use test_case::test_case;

#[test]
fn test_clip_args() {
    let d: Dictionary<'_> = [
        ("FunctionType".into(), 2i32.into()),
        (
            "Domain".into(),
            Object::Array(vec![
                0.0f32.into(),
                1.0f32.into(),
                Object::from(-2.0f32),
                2.0f32.into(),
            ]),
        ),
    ]
    .into_iter()
    .collect();

    let resolver = ObjectResolver::empty();
    let f = FunctionDict::new(None, &d, &resolver).unwrap();
    assert_eq!(f.clip_args(&[0.5, 0.0]), vec![0.5, 0.0]);
    assert_eq!(f.clip_args(&[-1.0, 100.0]), vec![0.0, 2.0]);
}

#[test]
fn test_clip_returns() {
    let d: Dictionary<'_> = [("/FunctionType".into(), 2i32.into())]
        .into_iter()
        .collect();

    let resolver = ObjectResolver::empty();
    let f = FunctionDict::new(None, &d, &resolver).unwrap();
    assert_eq!(f.clip_returns(vec![100.0, -100.0]), vec![100.0, -100.0]);
    assert_eq!(f.clip_returns(vec![]), vec![]);

    let d: Dictionary<'_> = [
        ("FunctionType".into(), 2i32.into()),
        (
            "Range".into(),
            Object::Array(vec![
                0.0f32.into(),
                1.0f32.into(),
                Object::from(-2.0f32),
                2.0f32.into(),
            ]),
        ),
    ]
    .into_iter()
    .collect();

    let f = FunctionDict::new(None, &d, &resolver).unwrap();
    assert_eq!(Type::ExponentialInterpolation, f.function_type().unwrap());
    assert_eq!(f.clip_returns(vec![0.5, 0.0]), vec![0.5, 0.0]);
    assert_eq!(f.clip_returns(vec![-1.0, 100.0]), vec![0.0, 2.0]);
}

#[test]
fn test_exponential_function() {
    let d: Dictionary<'_> = [
        ("FunctionType".into(), 2i32.into()),
        (
            "Domain".into(),
            Object::Array(vec![0.0f32.into(), 1.0f32.into()]),
        ),
        ("C0".into(), Object::Array(vec![0.1.into(), 0.2.into()])),
        ("C1".into(), Object::Array(vec![0.2.into(), 0.4.into()])),
        ("N".into(), 1.0f32.into()),
    ]
    .into_iter()
    .collect();

    let resolver = ObjectResolver::empty();
    let f = ExponentialInterpolationFunctionDict::new(None, &d, &resolver).unwrap();
    assert_eq!(f.call(&[0.0]).unwrap(), vec![0.1, 0.2]);
    assert_eq!(f.call(&[1.0]).unwrap(), vec![0.2, 0.4]);
    assert_eq!(f.call(&[0.5]).unwrap(), vec![0.15, 0.3]);
}

#[test]
fn stitching_find_function() {
    let bounds = vec![0.0f32, 0.5f32, 1.0f32];

    let resolver = ObjectResolver::empty();
    let f = StitchingFunctionDict::find_function;
    assert_eq!(f(&bounds[..], -1.0), 0);
    assert_eq!(f(&bounds[..], 0.0), 1);
    assert_eq!(f(&bounds[..], 0.5), 2);
    assert_eq!(f(&bounds[..], 1.0), 3);
    assert_eq!(f(&bounds[..], 2.0), 3);

    assert_eq!(f(&[], 2.0), 0);
}

#[test_case(0 => (0.0, 0.1))]
#[test_case(1 => (0.1, 0.5))]
#[test_case(2 => (0.5, 0.8))]
#[test_case(3 => (0.8, 1.0))]
fn stitching_sub_domain(idx: usize) -> (f32, f32) {
    let domain = Domain::new(0.0, 1.0);
    let bounds = vec![0.1f32, 0.5f32, 0.8f32];

    let act = StitchingFunctionDict::sub_domain(&domain, &bounds[..], idx);
    (act.start, act.end)
}

#[test]
fn stitching_sub_domain_empty_bounds() {
    let domain = Domain::new(0.0, 1.0);
    let bounds = vec![];

    assert_eq!(
        domain.clone(),
        StitchingFunctionDict::sub_domain(&domain, &bounds[..], 0)
    );
}

#[test]
fn interpolation() {
    let a = Domain::new(0.0, 1.0);
    let b = Domain::new(1.0, 0.0);

    assert_eq!(StitchingFunctionDict::interpolation(&a, &b, 0.0), 1.0);
    assert_eq!(StitchingFunctionDict::interpolation(&a, &b, 0.5), 0.5);
    assert_eq!(StitchingFunctionDict::interpolation(&a, &b, 1.0), 0.0);
}
