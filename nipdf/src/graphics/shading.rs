use anyhow::Result as AnyResult;
use educe::Educe;
use nipdf_macro::{pdf_object, TryFromIntObject};
use tiny_skia::{GradientStop, Shader, Transform};

use crate::{
    file::{Rectangle, ResourceDict},
    function::{default_domain, Domain, Function, FunctionDict, Type as FunctionType},
    graphics::{
        color_space::{ColorSpaceTrait, DeviceCMYK, DeviceGray, DeviceRGB},
        ColorArgs, ColorSpaceArgs,
    },
    object::{Object, ObjectValueError, PdfObject},
};

use super::{color_space::ColorSpace, Point};

#[derive(Copy, Clone, PartialEq, Eq, Debug, TryFromIntObject)]
pub enum ShadingType {
    Function = 1,
    Axial = 2,
    Radial = 3,
    FreeForm = 4,
    LatticeForm = 5,
    CoonsPatchMesh = 6,
    TensorProductPatchMesh = 7,
}

/// Return type of `AxialShadingDict::extend()`
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Extend(bool, bool);

impl Extend {
    pub fn new(begin: bool, end: bool) -> Self {
        Self(begin, end)
    }

    pub fn begin(&self) -> bool {
        self.0
    }

    pub fn end(&self) -> bool {
        self.1
    }
}

impl<'a> TryFrom<&Object<'a>> for Extend {
    type Error = ObjectValueError;

    fn try_from(obj: &Object) -> Result<Self, Self::Error> {
        let arr = obj.as_arr()?;
        if arr.len() != 2 {
            return Err(ObjectValueError::UnexpectedType);
        }
        Ok(Self(arr[0].as_bool()?, arr[1].as_bool()?))
    }
}

#[pdf_object(2i32)]
#[type_field("ShadingType")]
pub trait AxialShadingDictTrait {
    #[try_from]
    fn coords(&self) -> Rectangle;

    #[try_from]
    #[default_fn(default_domain)]
    fn domain(&self) -> Domain;

    #[one_or_more]
    #[nested]
    fn function(&self) -> Vec<FunctionDict<'a, 'b>>;

    #[try_from]
    #[or_default]
    fn extend(&self) -> Extend;
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct RadialCircle {
    pub point: Point,
    pub r: f32,
}

#[derive(Clone, PartialEq, Debug)]
pub struct RadialCoords {
    pub start: RadialCircle,
    pub end: RadialCircle,
}

impl<'a> TryFrom<&Object<'a>> for RadialCoords {
    type Error = ObjectValueError;

    fn try_from(obj: &Object) -> Result<Self, Self::Error> {
        let arr = obj.as_arr()?;
        if arr.len() != 6 {
            return Err(ObjectValueError::UnexpectedType);
        }
        Ok(Self {
            start: RadialCircle {
                point: Point {
                    x: arr[0].as_number()?,
                    y: arr[1].as_number()?,
                },
                r: arr[2].as_number()?,
            },
            end: RadialCircle {
                point: Point {
                    x: arr[3].as_number()?,
                    y: arr[4].as_number()?,
                },
                r: arr[5].as_number()?,
            },
        })
    }
}

#[pdf_object(3i32)]
#[type_field("ShadingType")]
pub trait RadialShadingDictTrait {
    #[try_from]
    fn coords(&self) -> RadialCoords;

    #[try_from]
    #[default_fn(default_domain)]
    fn domain(&self) -> Domain;

    #[one_or_more]
    #[nested]
    fn function(&self) -> Vec<FunctionDict<'a, 'b>>;

    #[try_from]
    #[or_default]
    fn extend(&self) -> Extend;
}

#[pdf_object(())]
pub trait ShadingDictTrait {
    #[try_from]
    fn shading_type(&self) -> ShadingType;

    #[try_from]
    fn color_space(&self) -> ColorSpaceArgs;

    #[try_from]
    fn background(&self) -> Option<ColorArgs>;

    #[try_from]
    fn b_box(&self) -> Option<Rectangle>;

    #[or_default]
    fn anti_alias(&self) -> bool;

    #[self_as]
    fn axial(&self) -> AxialShadingDict<'a, 'b>;

    #[self_as]
    fn radial(&self) -> RadialShadingDict<'a, 'b>;
}

fn build_linear_gradient_stops(
    domain: Domain,
    mut f: Vec<FunctionDict>,
) -> AnyResult<Vec<GradientStop>> {
    assert!(f.len() == 1, "todo: support functions");

    let f = f.pop().unwrap();
    fn create_stop<F: Function>(f: &F, x: f32) -> AnyResult<GradientStop> {
        let rv = f.call(&[x])?;
        // TODO: use current color space to check array length, and convert to skia color
        let color = match rv.len() {
            1 => DeviceGray().to_skia_color(&rv),
            3 => DeviceRGB().to_skia_color(&rv),
            4 => DeviceCMYK().to_skia_color(&rv),
            _ => unreachable!(),
        };
        Ok(GradientStop::new(x, color))
    }

    match f.function_type()? {
        FunctionType::ExponentialInterpolation => {
            let ef = f.exponential_interpolation()?;
            let eff = ef.func()?;
            assert_eq!(ef.n()?, 1f32, "Only linear gradient function supported");
            Ok(vec![
                create_stop(&eff, domain.start)?,
                create_stop(&eff, domain.end)?,
            ])
        }
        FunctionType::Stitching => {
            let sf = f.stitch()?;
            let sff = sf.func()?;
            let mut stops = Vec::with_capacity(sf.functions()?.len() + 1);
            stops.push(create_stop(&sff, domain.start)?);
            for t in sf.bounds()?.iter() {
                stops.push(create_stop(&sff, *t)?);
            }
            stops.push(create_stop(&f.func()?, domain.end)?);
            Ok(stops)
        }
        _ => {
            todo!("Unsupported function type: {:?}", f.function_type()?);
        }
    }
}

fn build_linear_gradient(d: &AxialShadingDict) -> AnyResult<Option<Shader<'static>>> {
    assert_eq!(d.extend()?, Extend::new(true, true), "Extend not supported");

    let coord = d.coords()?;
    let start = coord.left_lower();
    let end = coord.right_upper();
    let stops = build_linear_gradient_stops(d.domain()?, d.function()?)?;
    Ok(Some(tiny_skia::LinearGradient::new(
        start.into(),
        end.into(),
        stops,
        tiny_skia::SpreadMode::Pad,
        Transform::identity(),
    ))
    .unwrap())
}

#[derive(Educe)]
#[educe(PartialEq, Debug)]
pub struct Radial {
    pub start: RadialCircle,
    pub end: RadialCircle,
    #[educe(PartialEq(ignore))]
    #[educe(Debug(ignore))]
    pub function: Box<dyn Function>,
    pub domain: Domain,
    pub extend: Extend,
    pub color_space: ColorSpace,
}

pub enum Shading {
    Shader(Shader<'static>),
    Radial(Radial),
}

/// Return None if shading is not need to be rendered, such as Axial start point == end point.
pub fn build_shading<'a, 'b>(
    d: &ShadingDict<'a, 'b>,
    resources: &ResourceDict<'a, 'b>,
) -> AnyResult<Option<Shading>> {
    Ok(match d.shading_type()? {
        ShadingType::Axial => build_linear_gradient(&d.axial()?)?.map(Shading::Shader),
        ShadingType::Radial => build_radial(d, resources)?.map(Shading::Radial),
        t => todo!("{:?}", t),
    })
}

fn build_radial<'a, 'b>(
    d: &ShadingDict<'a, 'b>,
    resources: &ResourceDict<'a, 'b>,
) -> AnyResult<Option<Radial>> {
    let color_space = d.color_space()?;
    let color_space = ColorSpace::from_args(&color_space, resources.resolver(), Some(resources))?;

    let d = d.radial()?;
    let RadialCoords { start, end } = d.coords()?;
    if (start.r == 0.0 && end.r == 0.0) || start.r < 0. || end.r < 0. {
        return Ok(None);
    }

    let function = d.function()?.pop().unwrap().func()?;
    let domain = d.domain()?;
    let extend = d.extend()?;
    Ok(Some(Radial {
        color_space,
        start,
        end,
        function,
        domain,
        extend,
    }))
}

#[cfg(test)]
mod tests;
