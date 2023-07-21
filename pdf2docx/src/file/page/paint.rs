use crate::graphics::{Point, TransformMatrix};

use super::Operation;
use tiny_skia::{PathBuilder, Pixmap, Rect as SkiaRect, Shader, Stroke};

#[derive(Debug, Default, Clone)]
pub struct State {
    line_width: f32,
    path: PathBuilder,
    ctm: TransformMatrix,
}

impl State {
    fn set_line_width(&mut self, w: f32) {
        self.line_width = w;
    }

    fn set_ctm(&mut self, ctm: TransformMatrix) {
        self.ctm = ctm;
    }

    fn to_paint(&self) -> tiny_skia::Paint<'static> {
        let mut paint = tiny_skia::Paint::default();
        paint.set_color(tiny_skia::Color::BLACK);
        paint.shader = Shader::SolidColor(tiny_skia::Color::BLACK);
        // TODO: complete
        paint
    }

    fn to_stroke(&self) -> tiny_skia::Stroke {
        // TODO: complete
        let mut r = Stroke::default();
        r.width = self.line_width;
        r
    }

    fn to_transform(&self) -> tiny_skia::Transform {
        self.ctm.clone().into()
    }
}

#[derive(Debug)]
pub struct Render {
    canvas: Pixmap,
    stack: Vec<State>,
}

impl Render {
    pub fn new(canvas: Pixmap) -> Self {
        Self {
            canvas,
            stack: vec![State::default()],
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
            Operation::AppendBezierCurve1(Point { x: x1, y: y1 }, Point { x: x3, y: y3 }) => {
                self.current_mut()
                    .path
                    .cubic_to(*x1, *y1, *x3, *y3, *x3, *y3);
            }
            Operation::ClosePath => self.current_mut().path.close(),
            Operation::AppendRectangle(Point { x, y }, w, h) => self
                .current_mut()
                .path
                .push_rect(SkiaRect::from_xywh(*x, *y, *w, *h).unwrap()),

            // Path Painting Operation
            Operation::Stroke => {
                let state = self.current();
                let paint = state.to_paint();
                let stroke = state.to_stroke();
                let path = state.path.clone().finish().unwrap();
                let transform = state.to_transform();
                self.canvas
                    .stroke_path(&path, &paint, &stroke, transform, None);
            }
            _ => {
                eprintln!("unimplemented: {:?}", op);
            }
        }
    }
}
