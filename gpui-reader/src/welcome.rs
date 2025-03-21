use super::Open;
use gpui::{Context, DefaultColors, MouseButton, MouseDownEvent, Window, div, prelude::*};
use log::info;

pub(super) struct Welcome;

impl Welcome {
    pub fn new() -> Self {
        Self {}
    }

    fn on_mouse_down(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<'_, Self>) {
        info!("dispatching open action");
        cx.dispatch_action(&Open);
    }
}

impl Render for Welcome {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .text_color(gpui::DefaultColor::Text.color(&DefaultColors::dark()))
            .bg(gpui::DefaultColor::Background.color(&DefaultColors::dark()))
            .items_center()
            .justify_center()
            .size_full()
            .child(
                div()
                    .child("Open File...")
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down)),
            )
    }
}
