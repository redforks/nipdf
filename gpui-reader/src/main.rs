use gpui::{
    Application, Context, DefaultColors, Entity, ImageSource, KeyBinding, Menu, MenuItem,
    MouseButton, ObjectFit, RenderImage, SharedString, TitlebarOptions, Window, WindowOptions,
    actions, div, img, prelude::*,
};
use image::{Frame, RgbaImage};
use nipdf::file::File;
use nipdf_render::{RenderOptionBuilder, render_steps};
use prescript::{AnyWhatever, Result};
use smallvec::SmallVec;
use snafu::ResultExt as _;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

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
    image: Arc<RenderImage>,
}

fn open(path: impl AsRef<Path>, password: &str) -> Result<File> {
    let buf = std::fs::read(path).whatever_context("read file")?;
    File::parse(buf, password).whatever_context("Open pdf file")
}

impl MyApp {
    fn new(cx: &mut Context<'_, Self>) -> Self {
        let toolbox = cx.new(Toolbox::new);

        let f = open("/tmp/1.pdf", "").unwrap();
        let resolver = f.resolver().unwrap();
        let catalog = f.catalog(&resolver).unwrap();
        let page = &catalog.pages().unwrap()[0];
        let image = render_steps(page, RenderOptionBuilder::new().zoom(1.75), None, false)
            .whatever_context::<_, AnyWhatever>("render page")
            .unwrap();
        let mut frames = SmallVec::<[Frame; 1]>::with_capacity(1);
        frames.push(Frame::new(image));
        let image = Arc::new(RenderImage::new(frames));

        Self { toolbox, image }
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
                    .child(img(self.image.clone()).object_fit(ObjectFit::None)),
            )
    }
}

actions!(Self, [Quit]);

fn main() {
    colog::init();

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
