use crate::{
    graphics::{Color, LineCapStyle, LineJoinStyle, Point, RenderingIntent, TransformMatrix},
    object::FilterDecodedData,
};

use super::{Operation, Rectangle, ResourceDict};
use tiny_skia::{
    FillRule, FilterQuality, Mask, Paint, Path, PathBuilder, Pixmap, PixmapPaint, PixmapRef,
    Stroke, StrokeDash,
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

#[derive(Debug, Default, Clone)]
pub struct State {
    path: PathBuilder,
    ctm: TransformMatrix,
    fill_paint: Paint<'static>,
    stroke_paint: Paint<'static>,
    stroke: Stroke,
    mask: Option<Mask>,
}

impl State {
    fn new() -> Self {
        let mut r = Self::default();
        r.fill_paint.set_color(tiny_skia::Color::TRANSPARENT);
        r.stroke_paint.set_color(tiny_skia::Color::BLACK);
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
        self.ctm = ctm;
    }

    fn close_path(&mut self) {
        self.path.close();
    }

    fn move_to(&mut self, p: Point) {
        self.path.move_to(p.x, p.y);
    }

    fn line_to(&mut self, p: Point) {
        self.path.line_to(p.x, p.y);
    }

    fn curve_to(&mut self, p1: Point, p2: Point, p3: Point) {
        self.path.cubic_to(p1.x, p1.y, p2.x, p2.y, p3.x, p3.y);
    }

    fn curve_to_cur_point_as_control(&mut self, p2: Point, p3: Point) {
        let p1 = self.path.last_point().unwrap();
        self.curve_to(Point { x: p1.x, y: p2.y }, p2, p3);
    }

    fn curve_to_dest_point_as_control(&mut self, p1: Point, p3: Point) {
        self.curve_to(p1, p3, p3);
    }

    fn append_rect(&mut self, p: Point, w: f32, h: f32) {
        let r = Rectangle::from_xywh(p.x, p.y, w, h);
        self.path.push_rect(r.into());
    }

    fn end_path(&mut self) {
        self.path = PathBuilder::default();
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

    fn to_transform(&self) -> tiny_skia::Transform {
        self.ctm.into()
    }

    fn get_mask(&self) -> Option<&Mask> {
        self.mask.as_ref()
    }

    fn path(&self) -> Path {
        self.path.clone().finish().unwrap()
    }

    fn set_graphics_state(
        &mut self,
        nm: &crate::graphics::NameOfDict,
        resources: &ResourceDict<'_, '_>,
    ) {
        let resources = resources.ext_g_state();
        let res = resources.get(&nm.0);
        let Some(res) = res else {
            log::warn!("graphics state not found: {}", &nm.0);
            return;
        };

        if let Some(line_width) = res.line_width() {
            self.set_line_width(line_width);
        }

        if let Some(line_cap) = res.line_cap() {
            self.set_line_cap(line_cap);
        }

        if let Some(line_join) = res.line_join() {
            self.set_line_join(line_join);
        }

        if let Some(dash_pattern) = res.dash_pattern() {
            self.set_dash_pattern(&dash_pattern.0, dash_pattern.1);
        }

        if let Some(miter_limit) = res.miter_limit() {
            self.set_miter_limit(miter_limit);
        }

        if let Some(render_intent) = res.rendering_intent() {
            self.set_render_intent(render_intent);
        }

        if let Some(stroke_color) = res.stroke_adjustment() {
            eprintln!("stroke adjustment: {}", stroke_color);
        }

        if let Some(fill_alpha) = res.fill_alpha() {
            eprintln!("fill alpha: {}", fill_alpha);
        }

        if let Some(alpha_source_flag) = res.alpha_source_flag() {
            eprintln!("alpha source flag: {}", alpha_source_flag);
        }

        if let Some(text_knockout_flag) = res.text_knockout_flag() {
            eprintln!("text knockout flag: {}", text_knockout_flag);
        }
    }

    fn update_mask(&mut self, width: u32, height: u32, rule: FillRule) {
        let mut mask = self
            .mask
            .take()
            .unwrap_or_else(|| Mask::new(width, height).unwrap());
        mask.intersect_path(
            &self.path(),
            rule,
            true,
            tiny_skia::Transform::identity()
                .pre_scale(1.0, -1.0)
                .pre_translate(0.0, -(height as f32)),
        );
        self.mask = Some(mask);
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
pub struct Render {
    canvas: Pixmap,
    stack: Vec<State>,
    width: u32,
    height: u32,
}

impl Render {
    pub fn new(mut canvas: Pixmap, width: u32, height: u32) -> Self {
        // fill the whole canvas with white
        canvas.fill(tiny_skia::Color::WHITE);
        Self {
            canvas,
            stack: vec![State::new()],
            width,
            height,
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
            Operation::MoveToNext(p) => self.current_mut().move_to(*p),
            Operation::LineToNext(p) => self.current_mut().line_to(*p),
            Operation::AppendBezierCurve(p1, p2, p3) => self.current_mut().curve_to(*p1, *p2, *p3),
            Operation::AppendBezierCurve2(p2, p3) => {
                self.current_mut().curve_to_cur_point_as_control(*p2, *p3);
            }
            Operation::AppendBezierCurve1(p1, p3) => {
                self.current_mut().curve_to_dest_point_as_control(*p1, *p3);
            }
            Operation::ClosePath => self.current_mut().close_path(),
            Operation::AppendRectangle(p, w, h) => self.current_mut().append_rect(*p, *w, *h),

            // Path Painting Operation
            Operation::Stroke => self.stroke(),
            Operation::CloseAndStroke => self.close_and_stroke(),
            Operation::FillNonZero | Operation::FillNonZeroDeprecated => self.fill_path_non_zero(),
            Operation::FillEvenOdd => self.fill_path_even_odd(),
            Operation::FillAndStrokeNonZero => self.fill_and_stroke_non_zero(),
            Operation::FillAndStrokeEvenOdd => self.fill_and_stroke_even_odd(),
            Operation::CloseFillAndStrokeNonZero => self.close_fill_and_stroke_non_zero(),
            Operation::CloseFillAndStrokeEvenOdd => self.close_fill_and_stroke_even_odd(),
            Operation::EndPath => self.current_mut().end_path(),

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
            Operation::SetStrokeColor(color)
            | Operation::SetStrokeGray(color)
            | Operation::SetStrokeCMYK(color)
            | Operation::SetStrokeRGB(color) => self.current_mut().set_stroke_color(*color),
            Operation::SetFillColor(color)
            | Operation::SetFillGray(color)
            | Operation::SetFillCMYK(color)
            | Operation::SetFillRGB(color) => self.current_mut().set_fill_color(*color),

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
            &state.path(),
            paint,
            stroke,
            self.flip_y_axis(state.to_transform()),
            state.get_mask(),
        );
    }

    fn close_path(&mut self) {
        log::debug!("close_path");
        self.current_mut().close_path();
    }

    fn close_and_stroke(&mut self) {
        self.close_path();
        self.stroke();
    }

    fn fill_path_non_zero(&mut self) {
        let state = self.stack.last().unwrap();
        let paint = state.get_fill_paint();
        log::debug!("fill_path_non_zero: {:?}", paint);
        self.canvas.fill_path(
            &state.path(),
            paint,
            tiny_skia::FillRule::Winding,
            self.flip_y_axis(state.to_transform()),
            state.get_mask(),
        );
    }

    fn fill_path_even_odd(&mut self) {
        let state = self.stack.last().unwrap();
        let paint = state.get_fill_paint();
        log::debug!("fill_path_even_odd: {:?}", paint);
        self.canvas.fill_path(
            &state.path(),
            paint,
            tiny_skia::FillRule::EvenOdd,
            self.flip_y_axis(state.to_transform()),
            state.get_mask(),
        );
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

    fn flip_y_axis(&self, transform: tiny_skia::Transform) -> tiny_skia::Transform {
        transform
            .pre_scale(1.0, -1.0)
            .pre_translate(0.0, -(self.height as f32))
    }

    /// Paints the specified XObject. Only XObjectType::Image supported
    fn paint_x_object(&mut self, name: &crate::graphics::NameOfDict, resources: &ResourceDict) {
        let xobjects = resources.x_object();
        let xobject = xobjects.get(&name.0).unwrap();
        let img = xobject.as_image().expect("Only Image XObject supported");
        let img = img.decode(resources.d.resolver(), false).unwrap();
        let FilterDecodedData::Image(img) = img  else {
            panic!("Stream should decoded to image");
        };
        let state = self.stack.last().unwrap();

        let img = img.into_rgba8();
        let img = PixmapRef::from_bytes(img.as_raw(), img.width(), img.height()).unwrap();
        let mut paint = PixmapPaint::default();
        paint.quality = FilterQuality::Bicubic;
        let transform = tiny_skia::Transform::from_row(
            1.0 / img.width() as f32,
            0.0,
            0.0,
            -1.0 / img.height() as f32,
            0.0,
            1.0,
        )
        .post_concat(state.to_transform());
        let transform = self.flip_y_axis(transform);
        log::debug!("paint_x_object: {:?}", transform);

        // TODO: fix transform to move image to correct position
        // let transform = transform
        //     .pre_scale(1.0 / img.width() as f32, 1.0 / img.height() as f32)
        //     .pre_scale(1.0, -1.0)
        //     .pre_translate(0.0, -(self.height as f32))
        //     .pre_scale(1.0, -1.0)
        //     .pre_translate(0.0, -(self.height as f32));
        self.canvas
            .draw_pixmap(0, 0, img, &paint, transform, state.get_mask());
    }
}

#[cfg(test)]
mod tests;
