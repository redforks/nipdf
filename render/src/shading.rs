use crate::IntoSkia;
use anyhow::Result as AnyResult;
use educe::Educe;
use log::error;
use nipdf::{
    file::{Rectangle, ResourceDict},
    function::{Domain, Function, FunctionDict, Type as FunctionType},
    graphics::{
        color_space::{ColorSpace, ColorSpaceTrait},
        shading::{AxialCoords, RadialCoords, ShadingDict, ShadingType},
        trans::{IntoSkiaTransform, UserToLogicDeviceSpace},
        Extend, Point, RadialCircle,
    },
    object::PdfObject,
};
use std::rc::Rc;
use tiny_skia::{Color, GradientStop, LinearGradient, RadialGradient, Shader, Transform};

#[derive(PartialEq, Debug, Clone)]
pub struct Axial {
    pub start: Point,
    pub end: Point,
    pub extend: Extend,
    pub stops: Vec<(f32, Color)>,
    pub b_box: Option<Rectangle>,
}

impl Axial {
    pub fn to_skia(&self, transform: Transform, alpha: f32) -> Option<Shader<'static>> {
        LinearGradient::new(
            self.start.into_skia(),
            self.end.into_skia(),
            stops_to_skia(&self.stops[..], alpha),
            tiny_skia::SpreadMode::Pad,
            transform,
        )
    }
}

#[derive(Educe, Clone)]
#[educe(PartialEq, Debug)]
pub struct Radial {
    pub start: RadialCircle,
    pub end: RadialCircle,
    #[educe(PartialEq(ignore))]
    #[educe(Debug(ignore))]
    pub function: Rc<dyn Function>,
    pub domain: Domain,
    pub extend: Extend,
    pub color_space: ColorSpace,
    stops: Vec<(f32, Color)>,
}

impl Radial {
    pub fn to_skia(&self, transform: Transform, alpha: f32) -> Option<Shader<'static>> {
        RadialGradient::new(
            self.start.point.into_skia(),
            self.end.point.into_skia(),
            self.start.r.max(self.end.r),
            stops_to_skia(&self.stops[..], alpha),
            tiny_skia::SpreadMode::Pad,
            transform,
        )
    }
}

fn stops_to_skia(stops: &[(f32, Color)], alpha: f32) -> Vec<GradientStop> {
    stops
        .iter()
        .map(|(t, c)| {
            let mut c = *c;
            c.set_alpha(alpha);
            GradientStop::new(*t, c)
        })
        .collect()
}

#[derive(Clone, Debug)]
pub enum Shading {
    Axial(Axial),
    Radial(Radial),
}

impl Shading {
    pub fn to_skia(
        &self,
        transform: &UserToLogicDeviceSpace,
        alpha: f32,
    ) -> Option<Shader<'static>> {
        match self {
            Self::Axial(axial) => axial.to_skia(transform.into_skia(), alpha),
            Self::Radial(radial) => radial.to_skia(transform.into_skia(), alpha),
        }
    }
}

/// Return None if shading is not need to be rendered, such as Axial start point == end point.
pub fn build_shading<'a, 'b>(
    d: &ShadingDict<'a, 'b>,
    resources: &ResourceDict<'a, 'b>,
) -> AnyResult<Option<Shading>> {
    Ok(match d.shading_type()? {
        ShadingType::Axial => build_axial(d, resources)?.map(Shading::Axial),
        ShadingType::Radial => build_radial(d, resources)?.map(Shading::Radial),
        t => {
            error!("Shading not implemented: {:?}", t);
            None
        }
    })
}

fn build_axial(d: &ShadingDict, resources: &ResourceDict) -> AnyResult<Option<Axial>> {
    let axial = d.axial()?;
    let AxialCoords { start, end } = axial.coords()?;
    if start == end {
        return Ok(None);
    }

    let color_space = d.color_space()?;
    let color_space = ColorSpace::from_args(&color_space, resources.resolver(), Some(resources))?;
    let function = axial.function()?;

    let stops = build_stops(&color_space, axial.domain()?, function)?;
    Ok(Some(Axial {
        start,
        end,
        extend: axial.extend()?,
        stops,
        b_box: axial.b_box()?,
    }))
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
        stops: build_stops(&color_space, d.domain()?, d.function()?)?,
        color_space,
        start,
        end,
        function: function.into(),
        domain,
        extend,
    }))
}

fn build_stops(
    cs: &ColorSpace,
    domain: Domain,
    mut f: Vec<FunctionDict>,
) -> AnyResult<Vec<(f32, Color)>> {
    assert!(f.len() == 1, "todo: support functions");

    let f = f.pop().unwrap();
    fn create_stop<F: Function>(cs: &ColorSpace, f: &F, x: f32) -> AnyResult<(f32, Color)> {
        let rv = f.call(&[x])?;
        let color = cs.to_skia_color(&rv);
        Ok((x, color))
    }

    match f.function_type()? {
        FunctionType::ExponentialInterpolation => {
            let ef = f.exponential_interpolation()?;
            let eff = ef.func()?;
            assert_eq!(ef.n()?, 1f32, "Only linear gradient function supported");
            Ok(vec![
                create_stop(cs, &eff, domain.start)?,
                create_stop(cs, &eff, domain.end)?,
            ])
        }
        FunctionType::Stitching => {
            let sf = f.stitch()?;
            let sff = sf.func()?;
            let mut stops = Vec::with_capacity(sf.functions()?.len() + 1);
            stops.push(create_stop(cs, &sff, domain.start)?);
            for t in sf.bounds()?.iter() {
                stops.push(create_stop(cs, &sff, *t)?);
            }
            stops.push(create_stop(cs, &f.func()?, domain.end)?);
            Ok(stops)
        }
        FunctionType::Sampled => {
            let sf = f.sampled()?;
            let sff = sf.func()?;
            let t0 = euclid::default::Length::new(domain.start);
            let t1 = euclid::default::Length::new(domain.end);
            let len = sff.samples().min(256);
            let mut stops = Vec::with_capacity(len);
            for i in 0..=(len - 1) {
                stops.push(create_stop(
                    cs,
                    &sff,
                    t0.lerp(t1, i as f32 / (len - 1) as f32).0,
                )?);
            }
            Ok(stops)
        }
        _ => {
            todo!("Unsupported function type: {:?}", f.function_type()?);
        }
    }
}

#[cfg(test)]
mod tests;
