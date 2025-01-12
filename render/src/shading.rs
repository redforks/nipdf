use crate::{IntoSkia, Result, into_skia::to_skia_color};
use educe::Educe;
use log::error;
use nipdf::{
    file::{Rectangle, ResourceDict},
    function::{Domain, Function, FunctionDict, Type as FunctionType},
    graphics::{
        Extend, Point, RadialCircle,
        color_space::ColorSpace,
        shading::{AxialCoords, RadialCoords, ShadingDict, ShadingType},
        trans::UserToLogicDeviceSpace,
    },
    object::PdfObject,
};
use snafu::{OptionExt as _, ResultExt as _, ensure_whatever, whatever};
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
) -> Result<Option<Shading>> {
    Ok(
        match d.shading_type().whatever_context("get shading type")? {
            ShadingType::Axial => build_axial(d, resources)?.map(Shading::Axial),
            ShadingType::Radial => build_radial(d, resources)?.map(Shading::Radial),
            t => {
                error!("Shading not implemented: {:?}", t);
                None
            }
        },
    )
}

fn build_axial(d: &ShadingDict<'_, '_>, resources: &ResourceDict<'_, '_>) -> Result<Option<Axial>> {
    let axial = d.axial().whatever_context("get axial")?;
    let AxialCoords { start, end } = axial.coords().whatever_context("get coords")?;
    if start == end {
        return Ok(None);
    }

    let color_space = d.color_space().whatever_context("get color_space")?;
    let color_space = ColorSpace::from_args(&color_space, resources.resolver(), Some(resources))
        .whatever_context("parse color_space")?;
    let function = axial.function().whatever_context("get function")?;

    let stops = build_stops(
        &color_space,
        axial.domain().whatever_context("get domain")?,
        function,
    )
    .whatever_context("build axial stops")?;
    Ok(Some(Axial {
        start,
        end,
        extend: axial.extend().whatever_context("get extend")?,
        stops,
        b_box: axial.b_box().whatever_context("get b_box")?,
    }))
}

fn build_radial<'a, 'b>(
    d: &ShadingDict<'a, 'b>,
    resources: &ResourceDict<'a, 'b>,
) -> Result<Option<Radial>> {
    let color_space = d.color_space().whatever_context("get color_space")?;
    let color_space = ColorSpace::from_args(&color_space, resources.resolver(), Some(resources))
        .whatever_context("parse color_space")?;

    let d = d.radial().whatever_context("get radial")?;
    let RadialCoords { start, end } = d.coords().whatever_context("get coords")?;
    if (start.r == 0.0 && end.r == 0.0) || start.r < 0. || end.r < 0. {
        return Ok(None);
    }

    let function = d
        .function()
        .whatever_context("get functions")?
        .pop()
        .whatever_context("get last function")?
        .func()
        .whatever_context("get function")?;
    let domain = d.domain().whatever_context("get domain")?;
    let extend = d.extend().whatever_context("get extend")?;
    Ok(Some(Radial {
        stops: build_stops(
            &color_space,
            d.domain().whatever_context("get domain")?,
            d.function().whatever_context("get function")?,
        )?,
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
    mut f: Vec<FunctionDict<'_, '_>>,
) -> Result<Vec<(f32, Color)>> {
    ensure_whatever!(f.len() == 1, "todo: support functions");

    let f = f.pop().whatever_context("get last function")?;
    fn create_stop<F: Function>(cs: &ColorSpace, f: &F, x: f32) -> Result<(f32, Color)> {
        let rv = f.call(&[x]).whatever_context("exec function for stop")?;
        let color = to_skia_color(cs, &rv)?;
        Ok((x, color))
    }

    match f.function_type().whatever_context("get function type")? {
        FunctionType::ExponentialInterpolation => {
            let ef = f
                .exponential_interpolation()
                .whatever_context("get exponential_interpolation")?;
            let eff = ef.func().whatever_context("get func")?;
            ensure_whatever!(
                ef.n().whatever_context("get n")? == 1f32,
                "Only linear gradient function supported"
            );
            Ok(vec![
                create_stop(cs, &eff, domain.start)?,
                create_stop(cs, &eff, domain.end)?,
            ])
        }
        FunctionType::Stitching => {
            let sf = f.stitch().whatever_context("get stitch")?;
            let sff = sf.func().whatever_context("get func")?;
            let mut stops =
                Vec::with_capacity(sf.functions().whatever_context("get functions")?.len() + 1);
            stops.push(create_stop(cs, &sff, domain.start)?);
            for t in &sf.bounds().whatever_context("get bounds")? {
                stops.push(create_stop(cs, &sff, *t)?);
            }
            stops.push(create_stop(
                cs,
                &f.func().whatever_context("get func")?,
                domain.end,
            )?);
            Ok(stops)
        }
        FunctionType::Sampled => {
            let sf = f.sampled().whatever_context("get sampled")?;
            let sff = sf.func().whatever_context("get func")?;
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
            whatever!(
                "TODO: Unsupported function type: {:?}",
                f.function_type().whatever_context("get function type")?
            );
        }
    }
}

#[cfg(test)]
mod tests;
