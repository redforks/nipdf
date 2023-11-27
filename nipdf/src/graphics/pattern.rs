use crate::{
    file::{GraphicsStateParameterDict, Rectangle, ResourceDict},
    graphics::{shading::ShadingDict, trans::UserToDeviceIndependentSpace},
};
use ahash::HashMap;
use nipdf_macro::{pdf_object, TryFromIntObject};
use prescript::Name;

#[derive(Copy, Clone, PartialEq, Eq, Debug, TryFromIntObject)]
pub enum PatternType {
    Tiling = 1,
    Shading = 2,
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
    Colored = 1,
    Uncolored = 2,
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
    fn matrix(&self) -> UserToDeviceIndependentSpace;
}

#[pdf_object(2i32)]
#[type_field("PatternType")]
pub trait ShadingPatternDictTrait {
    #[nested]
    fn shading(&self) -> ShadingDict<'a, 'b>;

    #[try_from]
    #[or_default]
    fn matrix(&self) -> UserToDeviceIndependentSpace;

    #[nested]
    fn ext_g_state() -> HashMap<Name, GraphicsStateParameterDict<'a, 'b>>;
}
