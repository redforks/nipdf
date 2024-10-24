use super::*;
use crate::{
    file::{ObjectResolver, XRefTable},
    object::PdfObject,
    parser::parse_dict,
};
use assert_approx_eq::assert_approx_eq;
use mockall::predicate::eq;
use std::slice::from_ref;
use test_case::test_case;

#[test]
fn test_clip_args() {
    let signature = Signature {
        domain: Domains(vec![Domain::new(0.0, 1.0), Domain::new(-2.0, 2.0)]),
        range: None,
    };
    assert_eq!(
        signature.clip_args(&[0.5, 0.0]),
        tiny_vec![0.5_f32, 0.0_f32] as TinyVec<[f32; 4]>
    );
    assert_eq!(
        signature.clip_args(&[-1.0, 100.0]),
        tiny_vec![0.0_f32, 2.0_f32] as TinyVec<[f32; 4]>
    );
}

#[test]
fn test_clip_returns() {
    let signature = Signature {
        domain: Domains(vec![]),
        range: None,
    };
    assert_eq!(
        signature.clip_returns(tiny_vec![100.0, -100.0]),
        tiny_vec![100.0_f32, -100.0_f32] as FunctionValue
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
        signature.clip_returns(tiny_vec![0.5, 0.0]),
        FunctionValue::from([0.5f32, 0.0].as_slice()),
    );
    assert_eq!(
        signature.clip_returns(tiny_vec![-1.0, 100.0]),
        FunctionValue::from([0.0f32, 2.0].as_slice()),
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
    assert_eq!(f.call(&[0.0]).unwrap(), tiny_vec![0.1_f32, 0.2_f32]
        as FunctionValue);
    assert_eq!(f.call(&[1.0]).unwrap(), tiny_vec![0.2_f32, 0.4_f32]
        as FunctionValue);
    assert_eq!(f.call(&[0.5]).unwrap(), tiny_vec![0.15_f32, 0.3_f32]
        as FunctionValue);
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
    assert_eq!(f.call(&[0f32]).unwrap(), tiny_vec![0.2_f32, 0.4_f32]
        as FunctionValue);
}

#[test]
fn test_n_func() {
    assert!(NFunc::new(vec![]).is_err(), "check empty functions");

    let mut f1 = MockFunction::new();
    f1.expect_signature().return_const(Signature::new(
        Domains(vec![Domain::new(0.0, 1.0), Domain::new(2., 3.)]),
        Some(Domains(vec![Domain::new(0.0, 1.0)])),
    ));
    f1.expect_call()
        .with(eq(&[0.5_f32, 3.0_f32][..]))
        .returning(|_| Ok(FunctionValue::from(&[0.6_f32][..])));
    let mut f2 = MockFunction::new();
    f2.expect_signature().return_const(Signature::new(
        Domains(vec![Domain::new(0.0, 1.0), Domain::new(2., 3.)]),
        None,
    ));
    f2.expect_call()
        .with(eq(&[0.5_f32, 3.0_f32][..]))
        .returning(|_| Ok(FunctionValue::from(&[0.8_f32][..])));
    let f = NFunc::new(vec![Box::new(f1), Box::new(f2)]).unwrap();
    assert_eq!(
        f.signature().domain,
        Domains(vec![Domain::new(0.0, 1.0), Domain::new(2., 3.)])
    );
    assert!(f.signature().range.is_none());

    assert_eq!(
        f.call(&[0.5_f32, 3.0_f32][..]).unwrap(),
        FunctionValue::from(&[0.6_f32, 0.8_f32][..])
    )
}

#[test]
fn sampled_function_bits_per_sample_8() -> AnyResult<()> {
    let f = SampledFunction {
        bits_per_sample: 8,
        signature: Signature {
            domain: Domains(vec![Domain::new(0.0, 10.0), Domain::new(0.0, 2.0)]),
            range: Some(Domains(vec![Domain::new(0.0, 1.0)])),
        },
        encode: Domains(vec![Domain::new(0., 1.), Domain::new(0., 2.)]),
        decode: Domains(vec![Domain::new(0., 1.)]),
        samples: vec![
            /* (0, 0) */ 0, /* (1, 0) */ 192, /* (0, 1) */ 128,
            /* (1, 1) */ 64, /* (0, 2) */ 255, /* (1, 2) */ 32,
        ],
        size: vec![2, 3],
    };

    let cases = vec![
        ((0.0f32, 0.0f32), 0.0f32),
        ((1.0f32, 1.0f32), 128.0 / 255.0),
        ((4.0f32, 2.0f32), 255.0 / 255.0),
        ((6.0f32, 0.0f32), 192.0 / 255.0),
        ((7.0f32, 1.0f32), 64.0 / 255.0),
        ((7.0f32, 2.0f32), 32.0 / 255.0),
    ];
    for (args, exp) in cases {
        assert_approx_eq!(exp, f.call(&[args.0, args.1][..])?[0]);
    }
    Ok(())
}

#[test]
fn sampled_function_bits_per_sample_16() {
    let f = SampledFunction {
        bits_per_sample: 16,
        signature: Signature {
            domain: Domains(vec![Domain::new(0.0, 2.0)]),
            range: Some(Domains(vec![Domain::new(0.0, 1.0)])),
        },
        encode: Domains(vec![Domain::new(0., 2.)]),
        decode: Domains(vec![Domain::new(0., 1.)]),
        samples: vec![1, 2, 3, 4, 5, 6],
        size: vec![3],
    };

    let cases = vec![
        (0., 0x0102 as f32 / 65535.0),
        (1., 0x0304 as f32 / 65535.0),
        (2., 0x0506 as f32 / 65535.0),
    ];
    for (arg, exp) in cases {
        assert_approx_eq!(exp, f.call(from_ref(&arg)).unwrap()[0]);
    }
}
