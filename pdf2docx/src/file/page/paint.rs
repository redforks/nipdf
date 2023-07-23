use crate::graphics::{
    Color, LineCapStyle, LineJoinStyle, Point, RenderingIntent, TransformMatrix,
};

use super::Operation;
use tiny_skia::{Paint, Path, PathBuilder, Pixmap, Rect as SkiaRect, Shader, Stroke, StrokeDash};

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
}

impl State {
    fn new() -> Self {
        let mut r = Self::default();
        r.fill_paint.set_color(tiny_skia::Color::BLACK);
        r.fill_paint.shader = Shader::SolidColor(tiny_skia::Color::BLACK);
        r.stroke_paint.set_color(tiny_skia::Color::BLACK);
        r.stroke_paint.shader = Shader::SolidColor(tiny_skia::Color::BLACK);
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
        self.stroke_paint.shader = Shader::SolidColor(color.into());
    }

    fn set_fill_color(&mut self, color: Color) {
        self.fill_paint.shader = Shader::SolidColor(color.into());
    }

    fn set_ctm(&mut self, ctm: TransformMatrix) {
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
        self.path
            .push_rect(SkiaRect::from_xywh(p.x, p.y, w, h).unwrap());
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

    fn path(&self) -> Path {
        self.path.clone().finish().unwrap()
    }
}

#[derive(Debug)]
pub struct Render {
    canvas: Pixmap,
    stack: Vec<State>,
}

impl Render {
    pub fn new(mut canvas: Pixmap) -> Self {
        // fill the whole canvas with white
        canvas.fill(tiny_skia::Color::WHITE);
        Self {
            canvas,
            stack: vec![State::new()],
        }
    }

    fn push(&mut self) {
        self.stack.push(self.stack.last().unwrap().clone());
    }

    fn pop(&mut self) {
        self.stack.pop().unwrap();
    }

    fn current(&self) -> &State {
        self.stack.last().unwrap()
    }

    fn current_mut(&mut self) -> &mut State {
        self.stack.last_mut().unwrap()
    }

    pub fn into(self) -> Pixmap {
        self.canvas
    }

    pub fn exec(&mut self, op: &Operation) {
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

            // Color Operations
            Operation::SetStrokeColor(color)
            | Operation::SetStrokeGray(color)
            | Operation::SetStrokeCMYK(color)
            | Operation::SetStrokeRGB(color) => self.current_mut().set_stroke_color(*color),
            Operation::SetFillColor(color)
            | Operation::SetFillGray(color)
            | Operation::SetFillCMYK(color)
            | Operation::SetFillRGB(color) => self.current_mut().set_fill_color(*color),

            _ => {
                eprintln!("unimplemented: {:?}", op);
            }
        }
    }

    fn stroke(&mut self) {
        let state = self.stack.last().unwrap();
        self.canvas.stroke_path(
            &state.path(),
            state.get_fill_paint(),
            state.get_stroke(),
            state.to_transform(),
            None,
        );
    }

    fn close_path(&mut self) {
        self.current_mut().close_path();
    }

    fn close_and_stroke(&mut self) {
        self.close_path();
        self.stroke();
    }

    fn fill_path_non_zero(&mut self) {
        let state = self.stack.last().unwrap();
        self.canvas.fill_path(
            &state.path(),
            state.get_fill_paint(),
            tiny_skia::FillRule::Winding,
            state.to_transform(),
            None,
        );
    }

    fn fill_path_even_odd(&mut self) {
        let state = self.stack.last().unwrap();
        self.canvas.fill_path(
            &state.path(),
            state.get_fill_paint(),
            tiny_skia::FillRule::EvenOdd,
            state.to_transform(),
            None,
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
}

#[cfg(test)]
mod tests;
