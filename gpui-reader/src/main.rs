use std::path::PathBuf;

use gpui::{
    Application, Context, DefaultColors, Entity, KeyBinding, Menu, MenuItem, MouseButton,
    ObjectFit, SharedString, TitlebarOptions, Window, WindowOptions, actions, div, img, prelude::*,
};

struct ToolboxButton {
    text: SharedString,
    on_click: Box<dyn Fn(&mut Context<'_, Self>) + 'static>,
}

impl ToolboxButton {
    fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            on_click: Box::new(|_| ()),
        }
    }
}

impl Render for ToolboxButton {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        div().child(self.text.clone()).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| (this.on_click)(cx)),
        )
    }
}

struct Toolbox {
    buttons: Vec<Entity<ToolboxButton>>,
}

impl Toolbox {
    pub fn new(cx: &mut Context<'_, Self>) -> Self {
        let open = cx.new(|_| ToolboxButton::new("Open..."));
        let next = cx.new(|_| ToolboxButton::new("Next"));
        let prev = cx.new(|_| ToolboxButton::new("Prev"));
        Self {
            buttons: vec![open, next, prev],
        }
    }
}

impl Render for Toolbox {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<'_, Self>) -> impl IntoElement {
        div()
            .flex()
            .w_full()
            .gap_x_1()
            .children(self.buttons.clone())
    }
}

struct MyApp {
    toolbox: Entity<Toolbox>,
}

impl MyApp {
    fn new(cx: &mut Context<'_, Self>) -> Self {
        let toolbox = cx.new(Toolbox::new);
        Self { toolbox }
    }
}

impl Render for MyApp {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<'_, Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .gap_3()
            .px_2()
            .text_color(gpui::DefaultColor::Text.color(&DefaultColors::dark()))
            .bg(gpui::DefaultColor::Background.color(&DefaultColors::dark()))
            .child(self.toolbox.clone())
            .child(
                div()
                    .id("main")
                    .overflow_scroll()
                    .flex_grow()
                    .child(img(PathBuf::from("/tmp/new.png")).object_fit(ObjectFit::None)),
            )
    }
}

actions!(Self, [Quit]);

fn main() {
    Application::new().run(|cx| {
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.bind_keys([KeyBinding::new("ctrl-q", Quit, None)]);
        cx.set_menus(vec![Menu {
            name: "File".into(),
            items: vec![MenuItem::action("Quit", Quit)],
        }]);
        cx.open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("Nipdf Reader".into()),
                    appears_transparent: false,
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| cx.new(MyApp::new),
        )
        .unwrap();
    });
}
