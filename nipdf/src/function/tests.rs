use super::*;
use crate::{
    file::{ObjectResolver, XRefTable},
    object::PdfObject,
    parser::parse_dict,
};
use smallvec::smallvec;
use test_case::test_case;

#[test]
fn test_clip_args() {
    let signature = Signature {
        domain: Domains(vec![Domain::new(0.0, 1.0), Domain::new(-2.0, 2.0)]),
        range: None,
    };
    assert_eq!(
        signature.clip_args(&[0.5, 0.0]),
        smallvec![0.5_f32, 0.0_f32] as SmallVec<[f32; 4]>
    );
    assert_eq!(
        signature.clip_args(&[-1.0, 100.0]),
        smallvec![0.0_f32, 2.0_f32] as SmallVec<[f32; 4]>
    );
}

#[test]
fn test_clip_returns() {
    let signature = Signature {
        domain: Domains(vec![]),
        range: None,
    };
    assert_eq!(
        signature.clip_returns(smallvec![100.0, -100.0]),
        smallvec![100.0_f32, -100.0_f32] as FunctionValue
    );
    assert_eq!(
        signature.clip_returns(FunctionValue::new()),
        FunctionValue::new()
    );

    let signature = Signature {
        domain: Domains(vec![]),
        range: Some(Domains(vec![Domain::new(0.0, 1.0), Domain::new(-2.0, 2.0)])),
    };
    assert_eq!(
        signature.clip_returns(smallvec![0.5, 0.0]),
        FunctionValue::from_slice(&[0.5, 0.0]),
    );
    assert_eq!(
        signature.clip_returns(smallvec![-1.0, 100.0]),
        FunctionValue::from_slice(&[0.0, 2.0]),
    );
}

#[test]
fn test_exponential_function() {
    let (_, d) =
        parse_dict(br#"<</FunctionType 2/Domain[0 1]/C0[0.1 0.2]/C1[0.2 0.4]/N 1>>"#).unwrap();

    let xref = XRefTable::empty();
    let resolver = ObjectResolver::empty(&xref);
    let f = ExponentialInterpolationFunctionDict::new(None, &d, &resolver).unwrap();
    let f = f.func().unwrap();
    assert_eq!(
        f.call(&[0.0]).unwrap(),
        smallvec![0.1_f32, 0.2_f32] as FunctionValue
    );
    assert_eq!(
        f.call(&[1.0]).unwrap(),
        smallvec![0.2_f32, 0.4_f32] as FunctionValue
    );
    assert_eq!(
        f.call(&[0.5]).unwrap(),
        smallvec![0.15_f32, 0.3_f32] as FunctionValue
    );
}

#[test]
fn stitching_find_function() {
    let bounds = [0.0f32, 0.5f32, 1.0f32];

    let f = StitchingFunction::find_function;
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
    let bounds = [0.1f32, 0.5f32, 0.8f32];

    let act = StitchingFunction::sub_domain(&domain, &bounds[..], idx);
    (act.start, act.end)
}

#[test]
fn stitching_sub_domain_empty_bounds() {
    let domain = Domain::new(0.0, 1.0);
    let bounds = [];

    assert_eq!(
        domain.clone(),
        StitchingFunction::sub_domain(&domain, &bounds[..], 0)
    );
}

#[test]
fn interpolation() {
    let a = Domain::new(0.0, 1.0);
    let b = Domain::new(1.0, 0.0);

    assert_eq!(StitchingFunction::interpolation(&a, &b, 0.0), 1.0);
    assert_eq!(StitchingFunction::interpolation(&a, &b, 0.5), 0.5);
    assert_eq!(StitchingFunction::interpolation(&a, &b, 1.0), 0.0);
}

#[test]
fn stitching_function() {
    let (_, d) = parse_dict(
        br#"<</FunctionType 3/Domain[0 1]/Bounds[0.5]/Encode[1 0 1 0]
        /Functions[
            <</FunctionType 2/Domain[0 1]/C0[0.1 0.2]/C1[0.2 0.4]/N 1>>
            <</FunctionType 2/Domain[0 1]/C0[0.5 0.6]/C1[0.6 0.7]/N 1>>
        ]>>"#,
    )
    .unwrap();

    let xref = XRefTable::empty();
    let resolver = ObjectResolver::empty(&xref);
    let f = StitchingFunctionDict::new(None, &d, &resolver).unwrap();
    let f = f.func().unwrap();
    assert_eq!(
        f.call(&[0f32]).unwrap(),
        smallvec![0.2_f32, 0.4_f32] as FunctionValue
    );
}
