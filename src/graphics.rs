#[derive(Debug, Clone, PartialEq)]
pub struct TransformMatrix {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineCapStyle {
    Butt,
    Round,
    Square,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineJoinStyle {
    Miter,
    Round,
    Bevel,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RenderingIntent {
    AbsoluteColorimetric,
    RelativeColorimetric,
    Saturation,
    Perceptual,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Operation {
    SaveGraphicsState,
    RestoreGraphicsState,
    ModifyCTM(TransformMatrix),
    SetLineWidth(f32),
    SetLineCap(LineCapStyle),
    SetLineJoin(LineJoinStyle),
    SetMiterLimit(f32),
    SetDashPattern(Vec<f32>, f32),
    SetIntent(RenderingIntent),
    SetFlatness(f32),
    SetGraphicsStateParameters(String),
}
