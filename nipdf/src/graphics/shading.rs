use super::Point;
use crate::{
    ObjectValueError, Result,
    file::Rectangle,
    function::{Domain, Function, default_domain},
    graphics::{ColorArgs, ColorSpaceArgs},
    object::Object,
};
use nipdf_macro::{TryFromIntObject, pdf_object};
use prescript::sname;
use snafu::ResultExt;

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
        let arr = obj.as_arr()?;
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
        let arr = obj.as_arr()?;
        if arr.len() != 4 {
            return Err(ObjectValueError::UnexpectedType);
        }
        Ok(Self {
            start: Point::new(arr[0].number()?, arr[1].number()?),
            end: Point::new(arr[2].number()?, arr[3].number()?),
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

    #[try_from]
    #[or_default]
    fn extend(&self) -> Extend;

    #[deep_resolve_try_from]
    fn b_box(&self) -> Option<Rectangle>;
}

impl AxialShadingDict<'_, '_> {
    pub fn functions(&self) -> Result<Vec<Box<dyn Function>>> {
        self.d
            .zero_one_or_more(&sname("Function"))
            .whatever_context("get axial functions")
    }
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
        let arr = obj.as_arr()?;
        if arr.len() != 6 {
            return Err(ObjectValueError::UnexpectedType);
        }
        Ok(Self {
            start: RadialCircle {
                point: Point::new(arr[0].number()?, arr[1].number()?),
                r: arr[2].number()?,
            },
            end: RadialCircle {
                point: Point::new(arr[3].number()?, arr[4].number()?),
                r: arr[5].number()?,
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

    #[try_from]
    #[or_default]
    fn extend(&self) -> Extend;
}

impl RadialShadingDict<'_, '_> {
    pub fn functions(&self) -> Result<Vec<Box<dyn Function>>> {
        self.d
            .zero_one_or_more(&sname("Function"))
            .whatever_context("get radial functions")
    }
}

#[pdf_object(())]
pub trait ShadingDictTrait {
    #[try_from]
    fn shading_type(&self) -> ShadingType;

    #[try_from]
    fn color_space(&self) -> ColorSpaceArgs;

    #[try_from]
    fn background(&self) -> Option<ColorArgs>;

    #[deep_resolve_try_from]
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
