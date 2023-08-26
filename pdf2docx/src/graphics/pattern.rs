use pdf2docx_macro::{pdf_object, TryFromIntObject};
use std::collections::HashMap;

use super::{ColorSpace, Rectangle};
use crate::{
    file::{GraphicsStateParameterDict, ObjectResolver},
    function::{default_domain, Domain, FunctionDict},
    graphics::TransformMatrix,
    object::{Dictionary, Object, ObjectValueError, SchemaDict},
};

#[derive(Copy, Clone, PartialEq, Eq, Debug, TryFromIntObject)]
pub(crate) enum PatternType {
    Tiling = 1,
    Shading = 2,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, TryFromIntObject)]
pub(crate) enum ShadingType {
    Function = 1,
    Axial = 2,
    Radial = 3,
    FreeForm = 4,
    LatticeForm = 5,
    CoonsPatchMesh = 6,
    TensorProductPatchMesh = 7,
}

#[pdf_object(Some("Pattern"))]
pub(crate) trait PatternDictTrait {
    #[try_from]
    fn pattern_type(&self) -> PatternType;

    #[self_as]
    fn shading_pattern(&self) -> ShadingPatternDict<'a, 'b>;
}

#[pdf_object(2i32)]
#[type_field("PatternType")]
pub(crate) trait ShadingPatternDictTrait {
    #[nested]
    fn shading(&self) -> ShadingDict<'a, 'b>;

    #[try_from]
    #[or_default]
    fn matrix(&self) -> TransformMatrix;

    #[nested]
    fn ext_g_state() -> HashMap<String, GraphicsStateParameterDict<'a, 'b>>;
}

#[pdf_object(())]
pub(crate) trait ShadingDictTrait {
    #[try_from]
    fn shading_type(&self) -> ShadingType;

    #[try_from]
    fn color_space(&self) -> ColorSpace;

    #[try_from]
    fn b_box(&self) -> Option<Rectangle>;

    #[or_default]
    fn anti_alias(&self) -> bool;

    #[self_as]
    fn axial(&self) -> AxialShadingDict<'a, 'b>;
}

/// Return type of `AxialShadingDict::extend()`
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct AxialExtend(bool, bool);

impl AxialExtend {
    pub fn begin(&self) -> bool {
        self.0
    }

    pub fn end(&self) -> bool {
        self.1
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
pub(crate) trait AxialShadingDictTrait {
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
