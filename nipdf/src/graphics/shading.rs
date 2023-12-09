use super::{
    color_space::ColorSpace,
    trans::{IntoSkiaTransform, UserToLogicDeviceSpace},
    IntoSkia, Point,
};
use crate::{
    file::{Rectangle, ResourceDict},
    function::{default_domain, Domain, Function, FunctionDict, Type as FunctionType},
    graphics::{color_space::ColorSpaceTrait, ColorArgs, ColorSpaceArgs},
    object::{Object, ObjectValueError, PdfObject},
};
use anyhow::Result as AnyResult;
use educe::Educe;
use log::error;
use nipdf_macro::{pdf_object, TryFromIntObject};
use std::rc::Rc;
use tiny_skia::{Color, GradientStop, LinearGradient, RadialGradient, Shader, Transform};

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
            start: Point::new(arr[0].as_number()?, arr[1].as_number()?),
            end: Point::new(arr[2].as_number()?, arr[3].as_number()?),
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
                point: Point::new(arr[0].as_number()?, arr[1].as_number()?),
                r: arr[2].as_number()?,
            },
            end: RadialCircle {
                point: Point::new(arr[3].as_number()?, arr[4].as_number()?),
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

#[cfg(test)]
mod tests;
