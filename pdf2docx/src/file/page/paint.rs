use super::Operation;
use tiny_skia::Pixmap;

#[derive(Debug, Default, Clone)]
pub struct State {
    line_width: f32,
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
            Operation::SetLineWidth(width) => self.current_mut().set_line_width(*width),
            _ => {
                eprintln!("unimplemented: {:?}", op);
            }
        }
    }
}
