use super::*;
use crate::{
    file::{ObjectResolver, XRefTable},
    object::RootPdfObject as _,
    parser,
};
use assert_approx_eq::assert_approx_eq;
use std::slice::from_ref;
use test_case::test_case;
use winnow::{Parser as _, error::ContextError};

#[test]
fn test_clip_args() {
    let signature = Type23Signature {
        domain: Domains(vec![Domain::new(0.0, 1.0), Domain::new(-2.0, 2.0)]),
        range: None,
    };
    assert_eq!(signature.clip_args(&[0.5, 0.0]), tiny_vec![
        0.5_f32, 0.0_f32
    ]);
    assert_eq!(signature.clip_args(&[-1.0, 100.0]), tiny_vec![
        0.0_f32, 2.0_f32
    ]);
}

#[test]
fn test_clip_returns() {
    let signature = Type23Signature {
        domain: Domains(vec![]),
        range: None,
    };
    assert_eq!(signature.clip_returns(tiny_vec![100.0, -100.0]), tiny_vec![
        100.0_f32, -100.0_f32
    ]);
    assert_eq!(
        signature.clip_returns(FunctionValue::new()),
        FunctionValue::new()
    );

    let signature = Type23Signature {
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
    let d = parser::dict::<_, ContextError>
        .parse(br#"<</FunctionType 2/Domain[0 1]/C0[0.1 0.2]/C1[0.2 0.4]/N 1>>"#.as_slice())
        .unwrap();

    let xref = XRefTable::empty();
    let resolver = ObjectResolver::empty(&xref);
    let f = ExponentialInterpolationFunctionDict::new(1.into(), &d, &resolver).unwrap();
    let f = f.func().unwrap();
    assert_eq!(f.do_call(&[0.0]).unwrap(), tiny_vec![0.1_f32, 0.2_f32]);
    assert_eq!(f.do_call(&[1.0]).unwrap(), tiny_vec![0.2_f32, 0.4_f32]);
    assert_eq!(f.do_call(&[0.5]).unwrap(), tiny_vec![0.15_f32, 0.3_f32]);
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

    let act = StitchingFunction::sub_domain(domain, &bounds[..], idx);
    (act.start, act.end)
}

#[test]
fn stitching_sub_domain_empty_bounds() {
    let domain = Domain::new(0.0, 1.0);
    let bounds = [];

    assert_eq!(
        domain.clone(),
        StitchingFunction::sub_domain(domain, &bounds[..], 0)
    );
}

#[test]
fn interpolation() {
    let a = Domain::new(0.0, 1.0);
    let b = Domain::new(1.0, 0.0);

    assert_eq!(StitchingFunction::interpolation(a, b, 0.0), 1.0);
    assert_eq!(StitchingFunction::interpolation(a, b, 0.5), 0.5);
    assert_eq!(StitchingFunction::interpolation(a, b, 1.0), 0.0);
}

#[test]
fn sampled_function_bits_per_sample_8() -> Result<()> {
    let f = SampledFunction {
        bits_per_sample: 8,
        signature: Type04Signature {
            domain: Domains(vec![Domain::new(0.0, 10.0), Domain::new(0.0, 2.0)]),
            range: Domains(vec![Domain::new(0.0, 1.0)]),
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
        assert_approx_eq!(exp, f.do_call(&[args.0, args.1][..])?[0]);
    }
    Ok(())
}

#[test]
fn sampled_function_bits_per_sample_16() {
    let f = SampledFunction {
        bits_per_sample: 16,
        signature: Type04Signature {
            domain: Domains(vec![Domain::new(0.0, 2.0)]),
            range: Domains(vec![Domain::new(0.0, 1.0)]),
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
        assert_approx_eq!(exp, f.do_call(from_ref(&arg)).unwrap()[0]);
    }
}
