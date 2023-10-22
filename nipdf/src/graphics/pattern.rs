use ahash::HashMap;
use nipdf_macro::{pdf_object, TryFromIntObject};

use super::{ColorArgs, ColorSpaceArgs, Rectangle};
use crate::{
    file::{GraphicsStateParameterDict, ResourceDict},
    function::{default_domain, Domain, FunctionDict},
    graphics::TransformMatrix,
    object::{Object, ObjectValueError},
};

#[derive(Copy, Clone, PartialEq, Eq, Debug, TryFromIntObject)]
pub enum PatternType {
    Tiling = 1,
    Shading = 2,
}

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

#[pdf_object(Some("Pattern"))]
pub trait PatternDictTrait {
    #[try_from]
    fn pattern_type(&self) -> PatternType;

    #[self_as]
    fn tiling_pattern(&self) -> TilingPatternDict<'a, 'b>;

    #[self_as]
    fn shading_pattern(&self) -> ShadingPatternDict<'a, 'b>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, TryFromIntObject)]
pub enum TilingPaintType {
    Uncolored = 1,
    Colored = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, TryFromIntObject)]
pub enum TilingType {
    ConstantSpacing = 1,
    NoDistortion = 2,
    ConstantSpacingAndFasterTiling = 3,
}

#[pdf_object(1i32)]
#[type_field("PatternType")]
pub trait TilingPatternDictTrait {
    #[try_from]
    fn paint_type(&self) -> TilingPaintType;

    #[try_from]
    fn tiling_type(&self) -> TilingType;

    #[try_from]
    fn b_box(&self) -> Rectangle;

    fn x_step(&self) -> f32;

    fn y_step(&self) -> f32;

    #[nested]
    fn resources(&self) -> ResourceDict<'a, 'b>;

    #[try_from]
    #[or_default]
    fn matrix(&self) -> TransformMatrix;
}

#[pdf_object(2i32)]
#[type_field("PatternType")]
pub trait ShadingPatternDictTrait {
    #[nested]
    fn shading(&self) -> ShadingDict<'a, 'b>;

    #[try_from]
    #[or_default]
    fn matrix(&self) -> TransformMatrix;

    #[nested]
    fn ext_g_state() -> HashMap<String, GraphicsStateParameterDict<'a, 'b>>;
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
}

/// Return type of `AxialShadingDict::extend()`
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AxialExtend(bool, bool);

impl AxialExtend {
    pub fn new(begin: bool, end: bool) -> Self {
        Self(begin, end)
    }
}

impl<'a> TryFrom<&Object<'a>> for AxialExtend {
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

    #[nested]
    fn function(&self) -> FunctionDict<'a, 'b>;

    #[try_from]
    #[or_default]
    fn extend(&self) -> AxialExtend;
}
