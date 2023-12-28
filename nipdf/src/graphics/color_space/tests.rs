use super::*;
use crate::{
    file::XRefTable,
    function::{FunctionValue, MockFunction},
};
use assert_approx_eq::assert_approx_eq;
use mockall::predicate::*;
use prescript::name;
use test_case::test_case;
use tinyvec::tiny_vec;

#[test]
fn device_gray_to_rgb() {
    let color_space = DeviceGray;
    let rgba = color_space.to_rgba(&[0x80]);
    assert_eq!(rgba, [0x80, 0x80, 0x80, 0xff]);
}

#[test]
fn rgb_to_rgb() {
    let color_space = DeviceRGB;
    let color = [0x1, 0x2, 0x3];
    let rgba = color_space.to_rgba(&color);
    assert_eq!(rgba, [1, 2, 3, 255]);
}

#[test]
fn cmyk_to_rgb() {
    let color_space = DeviceCMYK;
    let color = [0, 0, 0, 0];
    let rgb = color_space.to_rgba(&color);
    assert_eq!(rgb, [255, 255, 255, 255]);

    let color = [255, 0, 0, 0];
    let rgb = color_space.to_rgba(&color);
    assert_eq!(rgb, [0, 173, 239, 255]);
}

fn to_u8(v: impl ColorCompConvertTo<u8>) -> u8 {
    v.into_color_comp()
}

#[test]
fn convert_color_comp_u8_to_f32() {
    assert_eq!(
        0.0f32,
        <u8 as ColorCompConvertTo<f32>>::into_color_comp(0_u8)
    );
    assert_eq!(
        1.0f32,
        <u8 as ColorCompConvertTo<f32>>::into_color_comp(255_u8)
    );
    assert_approx_eq!(
        0.5f32,
        ColorCompConvertTo::<f32>::into_color_comp(127_u8),
        0.01
    );
}

#[test]
fn convert_color_com_f32_to_u8() {
    assert_eq!(0_u8, to_u8(0.0f32));
    assert_eq!(255_u8, to_u8(1.0f32));
    assert_eq!(128_u8, to_u8(0.5f32)); // round integer part
    assert_eq!(0_u8, ColorCompConvertTo::<u8>::into_color_comp(-1.0f32));
    assert_eq!(255_u8, to_u8(33f32));
}

#[test]
fn test_color_to_rgba() {
    // DeviceGray u8 to u8 rgba
    let color_space = DeviceGray;
    assert_eq!(
        color_to_rgba::<_, u8, _>(&color_space, &[0x80]),
        [0x80, 0x80, 0x80, 0xff]
    );

    // DeviceGray u8 to f32 rgba
    let color_space = DeviceGray;
    assert_eq!(
        color_to_rgba::<_, f32, _>(&color_space, &[51]),
        [0.2f32, 0.2f32, 0.2f32, 1.0f32]
    );
}

#[test]
fn indexed_color_space() {
    let color_space = IndexedColorSpace {
        base: ColorSpace::DeviceRGB,
        data: vec![
            0x00, 0x00, 0x00, // black
            0xff, 0xff, 0xff, // white
        ],
    };
    assert_eq!(2, color_space.len());
    assert_eq!(color_space.to_rgba(&[0]), [0, 0, 0, 255]);
    assert_eq!(color_space.to_rgba(&[1]), [255, 255, 255, 255]);
}

#[test_case("DeviceRGB" => ColorSpace::DeviceRGB)]
#[test_case("DeviceGray" => ColorSpace::DeviceGray)]
#[test_case("DeviceCMYK" => ColorSpace::DeviceCMYK)]
#[test_case("Pattern" => ColorSpace::Pattern(Box::new(PatternColorSpace(None))))]
fn simple_color_space_from_args(nm: &str) -> ColorSpace<f32> {
    let empty_xref = XRefTable::empty();
    let resolver = ObjectResolver::empty(&empty_xref);
    ColorSpace::<f32>::from_args(&ColorSpaceArgs::Name(name(nm)), &resolver, None).unwrap()
}

#[test]
fn icc_based() -> AnyResult<()> {
    // use Alternate color space
    let buf = br#"
1 0 obj
[/ICCBased 2 0 R]
endobj
2 0 obj
<</Length 0/N 1/Alternate /DeviceGray>>
stream
endstream
endobj
"#;
    let xref = XRefTable::from_buf(buf);
    let resolver = ObjectResolver::new(buf, &xref, None);
    let args = ColorSpaceArgs::try_from(resolver.resolve(1)?)?;
    let color_space = ColorSpace::<f32>::from_args(&args, &resolver, None)?;
    assert_eq!(ColorSpace::DeviceGray, color_space);

    // if no Alternate, use Device{Gray, RGB, CMYK} by N value
    for (n, exp) in [
        (1, ColorSpace::DeviceGray),
        (3, ColorSpace::DeviceRGB),
        (4, ColorSpace::DeviceCMYK),
    ] {
        let buf = format!(
            r#"
1 0 obj
[/ICCBased 2 0 R]
endobj
2 0 obj
<</Length 0/N {}>>
stream
endstream
endobj
"#,
            n
        );
        let buf = buf.as_bytes();
        let xref = XRefTable::from_buf(buf);
        let resolver = ObjectResolver::new(buf, &xref, None);
        let args = ColorSpaceArgs::try_from(resolver.resolve(1)?)?;
        let color_space = ColorSpace::<f32>::from_args(&args, &resolver, None)?;
        assert_eq!(exp, color_space);
    }

    Ok(())
}

#[test]
fn separation_color_space() {
    let mut f = MockFunction::new();
    f.expect_call()
        .with(eq(vec![0.5f32]))
        .returning(|_| Ok(tiny_vec![0.1f32, 0.2f32, 0.3f32] as FunctionValue));
    let cs = SeparationColorSpace::<f32> {
        alt: ColorSpace::DeviceRGB,
        f: Rc::new(f),
    };

    assert_eq!(cs.to_rgba(&[0.5f32]), [0.1f32, 0.2f32, 0.3f32, 1.0]);
}

#[test]
fn separation() -> AnyResult<()> {
    // use Alternate color space
    let buf = br#"
1 0 obj
[/Separation /Black /DeviceGray 2 0 R]
endobj
2 0 obj
<</FunctionType 2/Domain [0 1]/N 1>>
endobj
"#;
    let xref = XRefTable::from_buf(buf);
    let resolver = ObjectResolver::new(buf, &xref, None);
    let args = ColorSpaceArgs::try_from(resolver.resolve(1)?)?;
    let color_space = ColorSpace::<f32>::from_args(&args, &resolver, None)?;
    assert_eq!(
        ColorSpace::Separation(Box::new(SeparationColorSpace {
            alt: ColorSpace::DeviceGray,
            f: Rc::new(MockFunction::new())
        })),
        color_space
    );
    Ok(())
}

#[test_case(b"1 0 obj
[/Indexed /DeviceRGB 1 2 0 R]
endobj
2 0 obj
<</Length 6>>
stream
\x01\x02\x03\x04\x05\x06
endstream
endobj
"; "stream")]
#[test_case(b"1 0 obj[/Indexed/DeviceRGB 1(\x01\x02\x03\x04\x05\x06)]endobj"; "Literal String")]
#[test_case(b"1 0 obj[/Indexed/DeviceRGB 1<010203040506>]endobj"; "Hex String")]
fn indexed(buf: &[u8]) -> AnyResult<()> {
    let xref = XRefTable::from_buf(buf);
    let resolver = ObjectResolver::new(buf, &xref, None);
    let args = ColorSpaceArgs::try_from(resolver.resolve(1)?).unwrap();
    let color_space = ColorSpace::<f32>::from_args(&args, &resolver, None).unwrap();
    assert_eq!(
        ColorSpace::Indexed(Box::new(IndexedColorSpace {
            base: ColorSpace::DeviceRGB,
            data: vec![1, 2, 3, 4, 5, 6]
        })),
        color_space
    );
    Ok(())
}

#[test]
fn cal_rgb_from_args() {
    let buf = br#" 
1 0 obj
[/CalRGB <</WhitePoint[0.9505 1.0 1.089]/BlackPoint[0.01 0.02 0.03]/Gamma[1.8 1.8 1.8]/Matrix[0.4497 0.2446 0.0252 0.3163 0.672 0.1412 0.1845 0.0833 0.9227]>>]
endobj
"#;
    let xref = XRefTable::from_buf(buf);
    let resolver = ObjectResolver::new(buf, &xref, None);
    let args = ColorSpaceArgs::try_from(resolver.resolve(1).unwrap()).unwrap();
    let color_space = ColorSpace::<f32>::from_args(&args, &resolver, None).unwrap();
    assert_eq!(
        ColorSpace::CalRGB(Box::new(CalRGBColorSpace {
            white_point: [0.9505, 1.0, 1.089],
            black_point: [0.01, 0.02, 0.03],
            gamma: [1.8, 1.8, 1.8],
            matrix: [
                0.4497, 0.2446, 0.0252, 0.3163, 0.672, 0.1412, 0.1845, 0.0833, 0.9227
            ],
        })),
        color_space
    );
}

#[test]
fn pattern_with_cs_from_args() {
    let buf = br#"1 0 obj
[/Pattern /DeviceRGB]
endobj
"#;
    let xref = XRefTable::from_buf(buf);
    let resolver = ObjectResolver::new(buf, &xref, None);
    let args = ColorSpaceArgs::try_from(resolver.resolve(1).unwrap()).unwrap();
    let color_space = ColorSpace::<f32>::from_args(&args, &resolver, None).unwrap();
    assert_eq!(
        ColorSpace::Pattern(Box::new(PatternColorSpace(Some(ColorSpace::DeviceRGB)))),
        color_space
    );
}

#[test]
fn cal_rgb_color_space() {
    let cs = CalRGBColorSpace {
        white_point: [0.9505, 1.0, 1.089],
        black_point: [0.01, 0.02, 0.03],
        gamma: [1.8, 1.8, 1.8],
        matrix: [
            0.4497, 0.2446, 0.0252, 0.3163, 0.672, 0.1412, 0.1845, 0.0833, 0.9227,
        ],
    };

    assert_eq!(cs.to_rgba(&[0., 1.0, 0.5]), [0., 1.0, 0.5, 1.0]);
}

#[test]
#[should_panic(expected = "Pattern CS base CS not set")]
fn pattern_no_base_panic_to_rgba() {
    let cs = PatternColorSpace::<f32>(None);
    cs.to_rgba(&[1.0, 0., 0., 0.]);
}

#[test]
fn pattern_color_space() {
    // no base color space
    let cs = PatternColorSpace::<f32>(None);
    assert_eq!(0, cs.components());

    // has base color space
    let cs = PatternColorSpace::<f32>(Some(ColorSpace::DeviceRGB));
    assert_eq!(3, cs.components());
    assert_eq!(cs.to_rgba(&[1.0, 0., 0., 0.]), [1.0, 0., 0., 1.]);
}

#[test]
fn device_n_from_args() -> AnyResult<()> {
    let buf = br#"1 0 obj
[/DeviceN [/foo] /DeviceRGB 2 0 R]
endobj
2 0 obj
<</FunctionType 2/Domain [0 1]/N 1>>
endobj
% has attribute
3 0 obj
[/DeviceN [/bar /None] /DeviceRGB 2 0 R <<>>]
endobj
% all color names are None
4 0 obj
[/DeviceN [/None /None] /DeviceRGB 2 0 R]
endobj
"#;
    let xref = XRefTable::from_buf(buf);
    let resolver = ObjectResolver::new(buf, &xref, None);

    let args = ColorSpaceArgs::try_from(resolver.resolve(1)?)?;
    let color_space = ColorSpace::<f32>::from_args(&args, &resolver, None)?;
    assert_eq!(1, color_space.components());
    assert!(matches!(color_space, ColorSpace::DeviceN(_)));

    let args = ColorSpaceArgs::try_from(resolver.resolve(3)?)?;
    let color_space = ColorSpace::<f32>::from_args(&args, &resolver, None)?;
    assert_eq!(2, color_space.components());
    assert!(matches!(color_space, ColorSpace::DeviceN(_)));

    let args = ColorSpaceArgs::try_from(resolver.resolve(4)?)?;
    let color_space = ColorSpace::<f32>::from_args(&args, &resolver, None);
    assert!(color_space.is_err());
    Ok(())
}

#[test]
fn lab_to_rgb() {
    let cs = LabColorSpace {
        white_point: [1.0, 1., 1.],
        ranges: [
            Domain::new(0., 100.),
            Domain::new(-128., 128.),
            Domain::new(-128., 128.),
        ],
        ..Default::default()
    };

    assert_eq!([0., 0., 0., 1.], cs.to_rgba(&[0., 0., 0.]));
    assert_eq!([0., 1., 0., 1.], cs.to_rgba(&[100., -128., 128.]));
    assert_eq!([6, 0, 0, 255], cs.to_rgba(&[0, 128, 128]));
    assert_eq!([0, 255, 0, 255], cs.to_rgba(&[255, 0, 255]));
}
