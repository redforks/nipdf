use crate::graphics::{LineCapStyle, LineJoinStyle, Point, RenderingIntent, TransformMatrix};

use super::Operation;
use tiny_skia::{Paint, Path, PathBuilder, Pixmap, Rect as SkiaRect, Shader, Stroke};

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

#[derive(Debug, Default, Clone)]
pub struct State {
    path: PathBuilder,
    ctm: TransformMatrix,
    paint: Paint<'static>,
    stroke: Stroke,
}

impl State {
    fn new() -> Self {
        let mut r = Self::default();
        r.paint.set_color(tiny_skia::Color::BLACK);
        r.paint.shader = Shader::SolidColor(tiny_skia::Color::BLACK);
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

    fn set_miter_limit(&mut self, limit: f32) {
        self.stroke.miter_limit = limit;
    }

    fn set_flatness(&mut self, flatness: f32) {
        log::info!("not implemented: flatness: {}", flatness);
    }

    fn set_render_intent(&mut self, intent: RenderingIntent) {
        log::info!("not implemented: render intent: {}", intent);
    }

    fn set_ctm(&mut self, ctm: TransformMatrix) {
        self.ctm = ctm;
    }

    fn get_paint(&self) -> &Paint<'static> {
        &self.paint
    }

    fn get_stroke(&self) -> &Stroke {
        &self.stroke
    }

    fn to_transform(&self) -> tiny_skia::Transform {
        self.ctm.clone().into()
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
            Operation::SetRenderIntent(intent) => self.current_mut().set_render_intent(*intent),
            Operation::SetFlatness(flatness) => self.current_mut().set_flatness(*flatness),

            // Special Graphics State Operations
            Operation::SaveGraphicsState => self.push(),
            Operation::RestoreGraphicsState => self.pop(),
            Operation::ModifyCTM(ctm) => self.current_mut().set_ctm(*ctm),

            // Path Construction Operations
            Operation::MoveToNext(Point { x, y }) => self.current_mut().path.move_to(*x, *y),
            Operation::LineToNext(Point { x, y }) => self.current_mut().path.line_to(*x, *y),
            Operation::AppendBezierCurve(
                Point { x: x1, y: y1 },
                Point { x: x2, y: y2 },
                Point { x: x3, y: y3 },
            ) => self
                .current_mut()
                .path
                .cubic_to(*x1, *y1, *x2, *y2, *x3, *y3),
            Operation::AppendBezierCurve2(Point { x: x2, y: y2 }, Point { x: x3, y: y3 }) => {
                let path = &mut self.current_mut().path;
                let p1 = path.last_point().unwrap();
                path.cubic_to(p1.x, p1.y, *x2, *y2, *x3, *y3);
            }
            Operation::AppendBezierCurve1(Point { x: x1, y: y1 }, Point { x: x3, y: y3 }) => self
                .current_mut()
                .path
                .cubic_to(*x1, *y1, *x3, *y3, *x3, *y3),
            Operation::ClosePath => self.current_mut().path.close(),
            Operation::AppendRectangle(Point { x, y }, w, h) => self
                .current_mut()
                .path
                .push_rect(SkiaRect::from_xywh(*x, *y, *w, *h).unwrap()),

            // Path Painting Operation
            Operation::Stroke => {
                let state = self.stack.last().unwrap();
                self.canvas.stroke_path(
                    &state.path(),
                    state.get_paint(),
                    state.get_stroke(),
                    state.to_transform(),
                    None,
                );
            }
            _ => {
                eprintln!("unimplemented: {:?}", op);
            }
        }
    }
}
