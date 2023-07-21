use crate::graphics::Point;

use super::Operation;
use tiny_skia::{PathBuilder, Pixmap, Rect as SkiaRect};

#[derive(Debug, Default, Clone)]
pub struct State {
    line_width: f32,
    path: PathBuilder,
}

impl State {
    fn set_line_width(&mut self, w: f32) {
        self.line_width = w;
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
            _ => {
                eprintln!("unimplemented: {:?}", op);
            }
        }
    }
}
