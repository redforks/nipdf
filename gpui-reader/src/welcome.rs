use gpui::{ClickEvent, Context, Window, div, prelude::*};
use ui::{
    App, Button, ButtonCommon as _, ButtonSize, Clickable as _, Label, LabelCommon as _, LabelSize,
};

pub(super) struct Welcome {
    on_open: Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>,
}

impl Welcome {
    pub fn new(on_open: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        Self {
            on_open: Box::new(on_open),
        }
    }

    fn on_open(&mut self, e: &ClickEvent, window: &mut Window, cx: &mut Context<'_, Self>) {
        (self.on_open)(e, window, cx);
    }
}

impl Render for Welcome {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        div()
            .flex()
            .p_5()
            .flex_col()
            .items_center()
            .justify_center()
            .size_full()
            .child(Label::new("Welcome to nipdf!").size(LabelSize::Large))
            .child(
                Button::new("open", "Open a pdf file")
                    .size(ButtonSize::Large)
                    .on_click(cx.listener(Self::on_open)),
            )
    }
}
