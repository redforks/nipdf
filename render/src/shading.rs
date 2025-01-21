use crate::{IntoSkia, Result, into_skia::to_skia_color};
use educe::Educe;
use log::error;
use nipdf::{
    file::{Rectangle, ResourceDict},
    function::{Domain, Function},
    graphics::{
        Extend, Point, RadialCircle,
        color_space::ColorSpace,
        shading::{AxialCoords, RadialCoords, ShadingDict, ShadingType},
        trans::UserToLogicDeviceSpace,
    },
    object::PdfObjectCore as _,
};
use snafu::{OptionExt as _, ResultExt as _};
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
            ShadingType::Axial => build_axial(d, resources)
                .whatever_context("build axial shading")?
                .map(Shading::Axial),
            ShadingType::Radial => build_radial(d, resources)
                .whatever_context("build radial shading")?
                .map(Shading::Radial),
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
    let function = axial
        .functions()
        .whatever_context("get function")?
        .pop()
        .whatever_context("get last function")?;

    let stops = build_stops(&color_space, &function).whatever_context("build axial stops")?;
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
        .functions()
        .whatever_context("get functions")?
        .pop()
        .whatever_context("get last function")?;
    let domain = d.domain().whatever_context("get domain")?;
    let extend = d.extend().whatever_context("get extend")?;
    Ok(Some(Radial {
        stops: build_stops(&color_space, &function)?,
        color_space,
        start,
        end,
        function: function.into(),
        domain,
        extend,
    }))
}

fn build_stops(cs: &ColorSpace, f: &dyn Function) -> Result<Vec<(f32, Color)>> {
    fn create_stop(cs: &ColorSpace, f: &dyn Function, x: f32) -> Result<(f32, Color)> {
        let rv = f.call(&[x]).whatever_context("exec function for stop")?;
        let color = to_skia_color(cs, &rv)?;
        Ok((x, color))
    }

    f.stops().map(move |t| create_stop(cs, f, t)).collect()
}

#[cfg(test)]
mod tests;
