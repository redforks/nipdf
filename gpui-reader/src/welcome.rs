use gpui::{App, Context, MouseButton, MouseDownEvent, Window, div, prelude::*};

pub(super) struct Welcome {
    on_open: Box<dyn Fn(&MouseDownEvent, &mut Window, &mut App)>,
}

impl Welcome {
    pub fn new() -> Self {
        Self {
            on_open: Box::new(|_, _, _| ()),
        }
    }

    pub fn on_open(
        mut self,
        listener: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_open = Box::new(listener);
        self
    }
}

impl Render for Welcome {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .size_full()
            .child(div().child("Open File...").cursor_pointer().on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e, w, cx| (this.on_open)(e, w, cx)),
            ))
    }
}
