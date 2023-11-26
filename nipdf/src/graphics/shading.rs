use super::{color_space::ColorSpace, Point};
use crate::{
    file::{Rectangle, ResourceDict},
    function::{default_domain, Domain, Function, FunctionDict, Type as FunctionType},
    graphics::{
        color_space::{ColorSpaceTrait, DeviceCMYK, DeviceGray, DeviceRGB},
        ColorArgs, ColorSpaceArgs,
    },
    object::{Object, ObjectValueError, PdfObject},
};
use anyhow::Result as AnyResult;
use educe::Educe;
use either::Either;
use nipdf_macro::{pdf_object, TryFromIntObject};
use tiny_skia::{GradientStop, LinearGradient, Shader, Transform};

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

impl TryFrom<&Object> for Extend {
    type Error = ObjectValueError;

    fn try_from(obj: &Object) -> Result<Self, Self::Error> {
        let arr = obj.arr()?;
        if arr.len() != 2 {
            return Err(ObjectValueError::UnexpectedType);
        }
        Ok(Self(arr[0].bool()?, arr[1].bool()?))
    }
}

#[derive(PartialEq, Debug, Clone, Copy)]
pub struct AxialCoords {
    pub start: Point,
    pub end: Point,
}

impl TryFrom<&Object> for AxialCoords {
    type Error = ObjectValueError;

    fn try_from(obj: &Object) -> Result<Self, Self::Error> {
        let arr = obj.arr()?;
        if arr.len() != 4 {
            return Err(ObjectValueError::UnexpectedType);
        }
        Ok(Self {
            start: Point {
                x: arr[0].as_number()?,
                y: arr[1].as_number()?,
            },
            end: Point {
                x: arr[2].as_number()?,
                y: arr[3].as_number()?,
            },
        })
    }
}

#[pdf_object(2i32)]
#[type_field("ShadingType")]
pub trait AxialShadingDictTrait {
    #[try_from]
    fn coords(&self) -> AxialCoords;

    #[try_from]
    #[default_fn(default_domain)]
    fn domain(&self) -> Domain;

    #[one_or_more]
    #[nested]
    fn function(&self) -> Vec<FunctionDict<'a, 'b>>;

    #[try_from]
    #[or_default]
    fn extend(&self) -> Extend;

    #[try_from]
    fn b_box(&self) -> Option<Rectangle>;
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

impl TryFrom<&Object> for RadialCoords {
    type Error = ObjectValueError;

    fn try_from(obj: &Object) -> Result<Self, Self::Error> {
        let arr = obj.arr()?;
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
            1 => DeviceGray.to_skia_color(&rv),
            3 => DeviceRGB.to_skia_color(&rv),
            4 => DeviceCMYK.to_skia_color(&rv),
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

fn build_axial(
    d: &ShadingDict,
    resources: &ResourceDict,
) -> AnyResult<Option<Either<Axial, SelfDrawnAxial>>> {
    let axial = d.axial()?;
    let AxialCoords { start, end } = axial.coords()?;
    if start == end {
        return Ok(None);
    }

    let function = axial.function()?;
    if function[0].function_type()? == FunctionType::Sampled {
        let color_space = d.color_space()?;
        let color_space =
            ColorSpace::from_args(&color_space, resources.resolver(), Some(resources))?;
        return Ok(Some(Either::Right(SelfDrawnAxial {
            start,
            end,
            extend: axial.extend()?,
            b_box: axial.b_box()?,
            function: function[0].func()?,
            domain: axial.domain()?,
            color_space,
        })));
    }

    let stops = build_linear_gradient_stops(axial.domain()?, function)?;
    Ok(Some(Either::Left(Axial {
        start,
        end,
        extend: axial.extend()?,
        stops,
        b_box: axial.b_box()?,
    })))
}

#[derive(PartialEq, Debug)]
pub struct Axial {
    pub start: Point,
    pub end: Point,
    pub extend: Extend,
    pub stops: Vec<GradientStop>,
    pub b_box: Option<Rectangle>,
}

impl Axial {
    pub fn into_skia(self) -> Shader<'static> {
        LinearGradient::new(
            self.start.into(),
            self.end.into(),
            self.stops,
            tiny_skia::SpreadMode::Pad,
            Transform::identity(),
        )
        .unwrap()
    }
}

/// Axial shading that drawn using function, not through gradient.
pub struct SelfDrawnAxial {
    pub start: Point,
    pub end: Point,
    pub extend: Extend,
    pub b_box: Option<Rectangle>,
    pub function: Box<dyn Function>,
    pub domain: Domain,
    pub color_space: ColorSpace,
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
    Axial(Axial),
    Radial(Radial),
    SelfDrawnAxial(SelfDrawnAxial),
}

/// Return None if shading is not need to be rendered, such as Axial start point == end point.
pub fn build_shading<'a, 'b>(
    d: &ShadingDict<'a, 'b>,
    resources: &ResourceDict<'a, 'b>,
) -> AnyResult<Option<Shading>> {
    Ok(match d.shading_type()? {
        ShadingType::Axial => build_axial(d, resources)?.map(|e| match e {
            Either::Left(axial) => Shading::Axial(axial),
            Either::Right(self_drawn_axial) => Shading::SelfDrawnAxial(self_drawn_axial),
        }),
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
