use crate::{
    function::{Domain, Function, FunctionDict, Type as FunctionType},
    graphics::{
        AxialExtend, AxialShadingDict, Color, ColorOrName, ColorSpace, ConvertFromObject,
        LineCapStyle, LineJoinStyle, PatternType, Point, RenderingIntent, ShadingType,
        TransformMatrix,
    },
    object::{Array, FilterDecodedData, Object},
};
use anyhow::Result as AnyResult;
use educe::Educe;
use itertools::Either;

use super::{Operation, Rectangle, ResourceDict};
use tiny_skia::{
    FillRule, FilterQuality, GradientStop, Mask, Paint, Path as SkiaPath, PathBuilder, Pixmap,
    PixmapPaint, PixmapRef, Point as SkiaPoint, Stroke, StrokeDash, Transform,
};

impl From<LineCapStyle> for tiny_skia::LineCap {
    fn from(cap: LineCapStyle) -> Self {
        match cap {
            LineCapStyle::Butt => tiny_skia::LineCap::Butt,
            LineCapStyle::Round => tiny_skia::LineCap::Round,
            LineCapStyle::Square => tiny_skia::LineCap::Square,
        }
    }
}

impl From<LineJoinStyle> for tiny_skia::LineJoin {
    fn from(join: LineJoinStyle) -> Self {
        match join {
            LineJoinStyle::Miter => tiny_skia::LineJoin::Miter,
            LineJoinStyle::Round => tiny_skia::LineJoin::Round,
            LineJoinStyle::Bevel => tiny_skia::LineJoin::Bevel,
        }
    }
}

impl From<Color> for tiny_skia::Color {
    fn from(color: Color) -> Self {
        match color {
            Color::Rgb(r, g, b) => tiny_skia::Color::from_rgba(r, g, b, 1.0).unwrap(),
            Color::Cmyk(c, m, y, k) => tiny_skia::Color::from_rgba(
                (1.0 - c) * (1.0 - k),
                (1.0 - m) * (1.0 - k),
                (1.0 - y) * (1.0 - k),
                1.0,
            )
            .unwrap(),
            Color::Gray(g) => tiny_skia::Color::from_rgba(g, g, g, 1.0).unwrap(),
        }
    }
}

impl From<Point> for SkiaPoint {
    fn from(p: Point) -> Self {
        Self::from_xy(p.x, p.y)
    }
}

#[derive(Debug, Clone)]
pub struct State {
    ctm: MatrixMapper,
    fill_paint: Paint<'static>,
    stroke_paint: Paint<'static>,
    stroke: Stroke,
    mask: Option<Mask>,
    fill_color_space: ColorSpace,
    stroke_color_space: ColorSpace,
}

impl State {
    /// height: height in user space coordinate
    fn new(option: &RenderOption) -> Self {
        let mut r = Self {
            ctm: MatrixMapper::new(
                option.height as f32,
                option.zoom,
                TransformMatrix::identity(),
            ),
            fill_paint: Paint::default(),
            stroke_paint: Paint::default(),
            stroke: Stroke::default(),
            mask: None,
            fill_color_space: ColorSpace::DeviceRGB,
            stroke_color_space: ColorSpace::DeviceRGB,
        };
        r.fill_paint.set_color(tiny_skia::Color::TRANSPARENT);
        r.stroke_paint.set_color(tiny_skia::Color::BLACK);

        r.set_line_cap(LineCapStyle::default());
        r.set_line_join(LineJoinStyle::default());
        r.set_miter_limit(10.0);
        r.set_dash_pattern(&[], 0.0);
        r.set_render_intent(RenderingIntent::default());

        r
    }

    fn set_line_width(&mut self, w: f32) {
        self.stroke.width = w;
    }

    fn set_line_cap(&mut self, cap: LineCapStyle) {
        self.stroke.line_cap = cap.into();
    }

    fn set_line_join(&mut self, join: LineJoinStyle) {
        self.stroke.line_join = join.into();
    }

    fn set_dash_pattern(&mut self, pattern: &[f32], phase: f32) {
        self.stroke.dash = StrokeDash::new(pattern.to_owned(), phase);
    }

    fn set_miter_limit(&mut self, limit: f32) {
        self.stroke.miter_limit = limit;
    }

    fn set_flatness(&mut self, flatness: f32) {
        log::info!("not implemented: flatness: {}", flatness);
    }

    fn set_render_intent(&mut self, intent: RenderingIntent) {
        log::info!("not implemented: render intent: {}", intent);
    }

    fn set_stroke_color(&mut self, color: Color) {
        log::debug!("set stroke color: {:?}", color);
        self.stroke_paint.set_color(color.into());
    }

    fn set_fill_color(&mut self, color: Color) {
        log::debug!("set fill color: {:?}", color);
        self.fill_paint.set_color(color.into());
    }

    fn set_ctm(&mut self, ctm: TransformMatrix) {
        log::debug!("set ctm: {:?}", ctm);
        self.ctm.set_ctm(ctm);
    }

    fn get_fill_paint(&self) -> &Paint<'static> {
        &self.fill_paint
    }

    fn get_stroke_paint(&self) -> &Paint<'static> {
        &self.stroke_paint
    }

    fn get_stroke(&self) -> &Stroke {
        &self.stroke
    }

    fn path_transform(&self) -> Transform {
        self.ctm.path_transform()
    }

    fn image_transform(&self, img_w: u32, img_h: u32) -> Transform {
        self.ctm.image_transform(img_w, img_h)
    }

    fn get_mask(&self) -> Option<&Mask> {
        self.mask.as_ref()
    }

    fn set_graphics_state(
        &mut self,
        nm: &crate::graphics::NameOfDict,
        resources: &ResourceDict<'_, '_>,
    ) {
        let resources = resources.ext_g_state().unwrap();
        let res = resources.get(&nm.0);
        let Some(res) = res else {
            log::warn!("graphics state not found: {}", &nm.0);
            return;
        };

        for key in res.d.dict().keys() {
            match key.as_ref() {
                "LW" => self.set_line_width(res.line_width().unwrap().unwrap()),
                "LC" => self.set_line_cap(res.line_cap().unwrap().unwrap()),
                "LJ" => self.set_line_join(res.line_join().unwrap().unwrap()),
                "ML" => self.set_miter_limit(res.miter_limit().unwrap().unwrap()),
                "RI" => self.set_render_intent(res.rendering_intent().unwrap().unwrap()),
                _ => log::info!("Unknown or unsupported ExtGState key: {}", key.as_ref()),
            }
        }
    }

    fn update_mask(&mut self, _width: u32, _height: u32, _rule: FillRule) {
        // let mut mask = self
        //     .mask
        //     .take()
        //     .unwrap_or_else(|| Mask::new(width, height).unwrap());
        // mask.intersect_path(
        //     &self.path.path(),
        //     rule,
        //     true,
        //     tiny_skia::Transform::identity()
        //         .pre_scale(1.0, -1.0)
        //         .pre_translate(0.0, -(height as f32)),
        // );
        // self.mask = Some(mask);
    }

    /// Apply current path to mask. Create mask if None, otherwise intersect with current path,
    /// using Winding fill rule.
    fn clip_non_zero(&mut self, width: u32, height: u32) {
        self.update_mask(width, height, FillRule::Winding);
    }

    /// Apply current path to mask. Create mask if None, otherwise intersect with current path,
    /// using Even-Odd fill rule.
    fn clip_even_odd(&mut self, width: u32, height: u32) {
        self.update_mask(width, height, FillRule::EvenOdd);
    }
}

#[derive(Debug)]
struct Path {
    path: Either<PathBuilder, SkiaPath>,
}

impl Default for Path {
    fn default() -> Self {
        Self {
            path: Either::Left(PathBuilder::new()),
        }
    }
}

impl Path {
    fn path_builder(&mut self) -> &mut PathBuilder {
        self.path.as_mut().left().unwrap()
    }

    fn close_path(&mut self) {
        self.path_builder().close();
    }

    fn move_to(&mut self, p: Point) {
        self.path_builder().move_to(p.x, p.y);
    }

    fn line_to(&mut self, p: Point) {
        self.path_builder().line_to(p.x, p.y);
    }

    fn curve_to(&mut self, p1: Point, p2: Point, p3: Point) {
        self.path_builder()
            .cubic_to(p1.x, p1.y, p2.x, p2.y, p3.x, p3.y);
    }

    fn curve_to_cur_point_as_control(&mut self, p2: Point, p3: Point) {
        let p1 = self.path_builder().last_point().unwrap();
        self.curve_to(Point { x: p1.x, y: p2.y }, p2, p3);
    }

    fn curve_to_dest_point_as_control(&mut self, p1: Point, p3: Point) {
        self.curve_to(p1, p3, p3);
    }

    fn append_rect(&mut self, p: Point, w: f32, h: f32) {
        let r = Rectangle::from_xywh(p.x, p.y, w, h);
        self.path_builder().push_rect(r.into());
    }

    /// Build path and clear the path builder
    fn finish(&mut self) -> &SkiaPath {
        if let Either::Left(_) = self.path {
            let temp = Either::Left(PathBuilder::new());
            let p = std::mem::replace(&mut self.path, temp);
            self.path = p.left_and_then(|p| Either::Right(p.finish().unwrap()));
        }

        match &self.path {
            Either::Left(_) => unreachable!(),
            Either::Right(p) => p,
        }
    }

    fn reset(&mut self) {
        let temp = Either::Left(PathBuilder::new());
        let p = std::mem::replace(&mut self.path, temp);
        self.path = p.right_and_then(|p| Either::Left(p.clear()));
    }

    fn clear(&mut self) {
        self.reset();
        self.path_builder().clear();
    }
}

/// Option for Render
#[derive(Debug, Educe)]
#[educe(Default)]
pub struct RenderOption {
    /// zoom level default to 1.0
    #[educe(Default = 1.0)]
    zoom: f32,
    width: u32,
    height: u32,
}

#[derive(Educe)]
#[educe(Default(new))]
pub struct RenderOptionBuilder(RenderOption);

impl RenderOptionBuilder {
    pub fn zoom(mut self, zoom: f32) -> Self {
        self.0.zoom = zoom;
        self
    }

    pub fn width(mut self, width: u32) -> Self {
        self.0.width = width;
        self
    }

    pub fn height(mut self, height: u32) -> Self {
        self.0.height = height;
        self
    }

    pub fn build(self) -> RenderOption {
        self.0
    }
}

#[derive(Debug)]
pub struct Render {
    canvas: Pixmap,
    stack: Vec<State>,
    width: u32,
    height: u32,
    path: Path,
}

impl Render {
    pub fn new(option: RenderOption) -> Self {
        let w = (option.width as f32 * option.zoom) as u32;
        let h = (option.height as f32 * option.zoom) as u32;

        let mut canvas = Pixmap::new(w, h).unwrap();
        // fill the whole canvas with white
        canvas.fill(tiny_skia::Color::WHITE);
        Self {
            canvas,
            stack: vec![State::new(&option)],
            width: w,
            height: h,
            path: Path::default(),
        }
    }

    fn push(&mut self) {
        self.stack.push(self.stack.last().unwrap().clone());
    }

    fn pop(&mut self) {
        self.stack.pop().unwrap();
    }

    fn current_mut(&mut self) -> &mut State {
        self.stack.last_mut().unwrap()
    }

    pub fn into(self) -> Pixmap {
        self.canvas
    }

    pub(crate) fn exec(&mut self, op: &Operation, resources: &ResourceDict) {
        match op {
            // General Graphics State Operations
            Operation::SetLineWidth(width) => self.current_mut().set_line_width(*width),
            Operation::SetLineCap(cap) => self.current_mut().set_line_cap(*cap),
            Operation::SetLineJoin(join) => self.current_mut().set_line_join(*join),
            Operation::SetMiterLimit(limit) => self.current_mut().set_miter_limit(*limit),
            Operation::SetDashPattern(pattern, phase) => {
                self.current_mut().set_dash_pattern(pattern, *phase)
            }
            Operation::SetRenderIntent(intent) => self.current_mut().set_render_intent(*intent),
            Operation::SetFlatness(flatness) => self.current_mut().set_flatness(*flatness),
            Operation::SetGraphicsStateParameters(nm) => {
                self.current_mut().set_graphics_state(nm, resources)
            }

            // Special Graphics State Operations
            Operation::SaveGraphicsState => self.push(),
            Operation::RestoreGraphicsState => self.pop(),
            Operation::ModifyCTM(ctm) => self.current_mut().set_ctm(*ctm),

            // Path Construction Operations
            Operation::MoveToNext(p) => self.path.move_to(*p),
            Operation::LineToNext(p) => self.path.line_to(*p),
            Operation::AppendBezierCurve(p1, p2, p3) => self.path.curve_to(*p1, *p2, *p3),
            Operation::AppendBezierCurve2(p2, p3) => {
                self.path.curve_to_cur_point_as_control(*p2, *p3);
            }
            Operation::AppendBezierCurve1(p1, p3) => {
                self.path.curve_to_dest_point_as_control(*p1, *p3);
            }
            Operation::ClosePath => self.path.close_path(),
            Operation::AppendRectangle(p, w, h) => self.path.append_rect(*p, *w, *h),

            // Path Painting Operation
            Operation::Stroke => self.stroke(),
            Operation::CloseAndStroke => self.close_and_stroke(),
            Operation::FillNonZero | Operation::FillNonZeroDeprecated => self.fill_path_non_zero(),
            Operation::FillEvenOdd => self.fill_path_even_odd(),
            Operation::FillAndStrokeNonZero => self.fill_and_stroke_non_zero(),
            Operation::FillAndStrokeEvenOdd => self.fill_and_stroke_even_odd(),
            Operation::CloseFillAndStrokeNonZero => self.close_fill_and_stroke_non_zero(),
            Operation::CloseFillAndStrokeEvenOdd => self.close_fill_and_stroke_even_odd(),
            Operation::EndPath => self.end_path(),

            // Clipping Path Operations
            Operation::ClipNonZero => {
                let (w, h) = (self.width, self.height);
                self.current_mut().clip_non_zero(w, h);
            }
            Operation::ClipEvenOdd => {
                let (w, h) = (self.width, self.height);
                self.current_mut().clip_even_odd(w, h);
            }

            // Color Operations
            Operation::SetStrokeColorSpace(space) => self.current_mut().stroke_color_space = *space,
            Operation::SetFillColorSpace(space) => self.current_mut().fill_color_space = *space,
            Operation::SetStrokeColor(color)
            | Operation::SetStrokeGray(color)
            | Operation::SetStrokeCMYK(color)
            | Operation::SetStrokeRGB(color) => self.current_mut().set_stroke_color(*color),
            Operation::SetFillColor(color)
            | Operation::SetFillGray(color)
            | Operation::SetFillCMYK(color)
            | Operation::SetFillRGB(color) => self.current_mut().set_fill_color(*color),
            Operation::SetFillColorOrWithPattern(name) => {
                self.set_fill_color_or_pattern(name, resources).unwrap()
            }

            // XObject Operation
            Operation::PaintXObject(name) => self.paint_x_object(name, resources),

            _ => {
                eprintln!("unimplemented: {:?}", op);
            }
        }
    }

    fn stroke(&mut self) {
        let state = self.stack.last().unwrap();
        let paint = state.get_stroke_paint();
        let stroke = state.get_stroke();
        log::debug!("stroke: {:?} {:?}", paint, stroke);
        self.canvas.stroke_path(
            self.path.finish(),
            paint,
            stroke,
            state.path_transform(),
            state.get_mask(),
        );
        self.path.reset();
    }

    fn end_path(&mut self) {
        self.path.clear();
    }

    fn close_path(&mut self) {
        log::debug!("close_path");
        self.path.close_path();
    }

    fn close_and_stroke(&mut self) {
        self.close_path();
        self.stroke();
    }

    fn _fill(&mut self, fill_rule: FillRule) {
        let state = self.stack.last().unwrap();
        let paint = state.get_fill_paint();
        log::debug!("fill: {:?}/{:?}", paint, fill_rule);
        self.canvas.fill_path(
            self.path.finish(),
            paint,
            fill_rule,
            state.path_transform(),
            state.get_mask(),
        );
        self.path.reset();
    }

    fn fill_path_non_zero(&mut self) {
        self._fill(FillRule::Winding);
    }

    fn fill_path_even_odd(&mut self) {
        self._fill(FillRule::EvenOdd);
    }

    fn fill_and_stroke_non_zero(&mut self) {
        self.fill_path_non_zero();
        self.stroke();
    }

    fn fill_and_stroke_even_odd(&mut self) {
        self.fill_path_even_odd();
        self.stroke();
    }

    fn close_fill_and_stroke_non_zero(&mut self) {
        self.close_path();
        self.fill_and_stroke_non_zero();
    }

    fn close_fill_and_stroke_even_odd(&mut self) {
        self.close_path();
        self.fill_and_stroke_even_odd();
    }

    /// Paints the specified XObject. Only XObjectType::Image supported
    fn paint_x_object(&mut self, name: &crate::graphics::NameOfDict, resources: &ResourceDict) {
        let xobjects = resources.x_object().unwrap();
        let xobject = xobjects.get(&name.0).unwrap();
        let img = xobject.as_image().expect("Only Image XObject supported");
        let img = img.decode(resources.d.resolver(), false).unwrap();
        let FilterDecodedData::Image(img) = img else {
            panic!("Stream should decoded to image");
        };
        let state = self.stack.last().unwrap();

        let img = img.into_rgba8();
        let img = PixmapRef::from_bytes(img.as_raw(), img.width(), img.height()).unwrap();
        let paint = PixmapPaint {
            quality: FilterQuality::Bilinear,
            ..Default::default()
        };
        let transform = state.image_transform(img.width(), img.height());
        log::debug!("paint_x_object: {:?}", transform);

        self.canvas
            .draw_pixmap(0, 0, img, &paint, transform, state.get_mask());
    }

    fn set_fill_color_or_pattern(
        &mut self,
        color_or_name: &crate::graphics::ColorOrName,
        resources: &ResourceDict,
    ) -> AnyResult<()> {
        let ColorOrName::Name(name) = color_or_name else {
            panic!("Only Name supported");
        };

        let pattern = resources.pattern()?;
        let pattern = pattern.get(name.as_str()).unwrap();
        if pattern.pattern_type()? != PatternType::Shading {
            log::error!("Only shading pattern supported");
            return Ok(());
        }
        let pattern = pattern.shading_pattern()?;
        assert_eq!(
            pattern.matrix()?,
            TransformMatrix::identity(),
            "matrix not supported"
        );
        assert!(pattern.ext_g_state()?.is_empty(), "ExtGState not supported");

        let shading = pattern.shading()?;
        assert!(shading.b_box()?.is_none(), "TODO: support BBox of shading");
        assert!(
            shading.background()?.is_none(),
            "TODO: support Background of shading, paint background before shading"
        );

        assert_eq!(shading.shading_type()?, ShadingType::Axial);
        let axial = shading.axial()?;
        assert_eq!(
            axial.extend()?,
            AxialExtend::new(true, true),
            "Extend not supported"
        );
        let pattern = build_linear_gradient(&axial)?;
        self.stack.last_mut().unwrap().fill_paint.shader = pattern;
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct MatrixMapper {
    // height of user space coordinate
    height: f32,
    zoom: f32,
    ctm: TransformMatrix,
}

impl MatrixMapper {
    /// height: height of user space coordinate
    pub fn new(height: f32, zoom: f32, ctm: TransformMatrix) -> Self {
        Self { height, zoom, ctm }
    }

    pub fn set_ctm(&mut self, ctm: TransformMatrix) {
        self.ctm = ctm;
    }

    pub fn path_transform(&self) -> Transform {
        self.flip_y(self.ctm.into())
    }

    fn flip_y(&self, t: Transform) -> Transform {
        t.pre_scale(self.zoom, -self.zoom)
            .pre_translate(0.0, -self.height)
    }

    pub fn image_transform(&self, img_w: u32, img_h: u32) -> Transform {
        Transform::from_row(
            self.ctm.sx / img_w as f32 * self.zoom,
            0.0,
            0.0,
            self.ctm.sy / img_h as f32 * self.zoom,
            self.ctm.tx * self.zoom,
            self.height * self.zoom - self.ctm.ty * self.zoom - self.ctm.sy * self.zoom,
        )
    }
}

fn build_linear_gradient_stops(domain: Domain, f: FunctionDict) -> AnyResult<Vec<GradientStop>> {
    fn create_stop<F: Function>(f: &F, x: f32) -> AnyResult<GradientStop> {
        let rv = f.call(&[x])?;
        let mut arr = rv.into_iter().map(Object::Number).collect::<Array>();
        // TODO: Optimize speed of convert Vec<f32> to Color instead of using Object array
        let color = Color::convert_from_object(&mut arr)?;
        Ok(GradientStop::new(x, color.into()))
    }

    match f.function_type()? {
        FunctionType::ExponentialInterpolation => {
            let ef = f.exponential_interpolation()?;
            assert_eq!(ef.n()?, 1f32, "Only linear gradient function supported");
            Ok(vec![
                create_stop(&ef, domain.start)?,
                create_stop(&ef, domain.end)?,
            ])
        }
        FunctionType::Stitching => {
            let sf = f.stitch()?;
            let mut stops = Vec::with_capacity(sf.functions()?.len() + 1);
            stops.push(create_stop(&sf, domain.start)?);
            let functions = sf.functions()?;
            for f in &functions {
                let ef = f.exponential_interpolation()?; // only support exponential interpolation
                assert_eq!(ef.n()?, 1f32, "Only linear gradient function supported");
            }
            for t in sf.bounds()?.iter() {
                stops.push(create_stop(&sf, *t)?);
            }
            stops.push(create_stop(&f, domain.end)?);
            Ok(stops)
        }
        _ => {
            todo!("Unsupported function type: {:?}", f.function_type()?);
        }
    }
}

fn build_linear_gradient(shading: &AxialShadingDict) -> AnyResult<tiny_skia::Shader<'static>> {
    let coords = shading.coords()?;
    let start = coords.left_lower();
    let end = coords.right_upper();
    let stops = build_linear_gradient_stops(shading.domain()?, shading.function()?)?;
    Ok(tiny_skia::LinearGradient::new(
        start.into(),
        end.into(),
        stops,
        tiny_skia::SpreadMode::Pad,
        Transform::identity(),
    )
    .unwrap())
}

#[cfg(test)]
mod tests;
