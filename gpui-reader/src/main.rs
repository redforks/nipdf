use either::Either;
use gpui::{
    App, AppContext, Application, ClickEvent, Context, DefaultColors, Entity, KeyBinding, Menu,
    MenuItem, MouseButton, MouseDownEvent, ObjectFit, RenderImage, SharedString, TitlebarOptions,
    Window, WindowOptions, actions, div, img, prelude::*,
};
use image::Frame;
use log::info;
use nipdf::file::File;
use nipdf_render::{RenderOptionBuilder, render_steps};
use prescript::AnyWhatever;
use smallvec::SmallVec;
use snafu::ResultExt as _;
use std::{path::Path, sync::Arc};
use theme::{LoadThemes, SystemAppearance};

mod welcome;
use welcome::Welcome;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

struct ToolboxButton {
    text: SharedString,
    on_click: Box<dyn Fn(&MouseDownEvent, &mut Window, &mut App)>,
}

impl ToolboxButton {
    fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            on_click: Box::new(|_, _, _| ()),
        }
    }

    fn on_click(
        mut self,
        listener: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Box::new(listener);
        self
    }
}

impl Render for ToolboxButton {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        div().child(self.text.clone()).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, e, w, cx| (this.on_click)(e, w, cx)),
        )
    }
}

#[derive(educe::Educe)]
#[educe(Default(new))]
struct Toolbox {
    buttons: Vec<Entity<ToolboxButton>>,
}

impl Toolbox {
    pub fn child(mut self, btn: Entity<ToolboxButton>) -> Self {
        self.buttons.push(btn);
        self
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

struct PdfViewer {
    image: Arc<RenderImage>,
    file: File,
    cur_page: usize,
}

impl PdfViewer {
    fn new(f: impl AsRef<Path>) -> Self {
        Self::open_file(f, 0)
    }

    fn open_file(file: impl AsRef<Path>, page_no: usize) -> Self {
        let buf = std::fs::read(file)
            .whatever_context::<_, AnyWhatever>("read file")
            .unwrap();
        let f = File::parse(buf, "")
            .whatever_context::<_, AnyWhatever>("Open pdf file")
            .unwrap();
        let (image, page_no) = Self::goto_page(&f, page_no);
        Self {
            image,
            file: f,
            cur_page: page_no,
        }
    }

    fn assign(&mut self, (image, page_no): (Arc<RenderImage>, usize)) {
        self.image = image;
        self.cur_page = page_no;
    }

    fn open(&mut self, file: impl AsRef<Path>) {
        let f = Self::open_file(file, 0);
        self.file = f.file;
        self.image = f.image;
        self.cur_page = 0;
    }

    fn goto_page(file: &File, page_no: usize) -> (Arc<RenderImage>, usize) {
        let resolver = file.resolver().unwrap();
        let catalog = file.catalog(&resolver).unwrap();
        let pages = catalog.pages().unwrap();
        let page_no = page_no.clamp(0, pages.len() - 1);

        let page = &pages[page_no];
        let image = render_steps(page, RenderOptionBuilder::new().zoom(1.75), None, false)
            .whatever_context::<_, AnyWhatever>("render page")
            .unwrap();
        let mut frames = SmallVec::<[Frame; 1]>::with_capacity(1);
        frames.push(Frame::new(image));
        let image = Arc::new(RenderImage::new(frames));
        (image, page_no)
    }

    fn next_page(&mut self) {
        self.assign(Self::goto_page(&self.file, self.cur_page + 1));
    }

    fn prev_page(&mut self) {
        self.assign(Self::goto_page(&self.file, self.cur_page.saturating_sub(1)));
    }
}

impl Render for PdfViewer {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<'_, Self>) -> impl IntoElement {
        div()
            .id("main")
            .overflow_scroll()
            .flex_grow()
            .child(img(self.image.clone()).object_fit(ObjectFit::None))
    }
}

struct Viewer {
    toolbox: Entity<Toolbox>,
    viewer: Entity<PdfViewer>,
}

impl Viewer {
    fn new(cx: &mut Context<'_, Self>, file: impl AsRef<Path>) -> Self {
        let viewer = cx.new(|_| PdfViewer::new(file));
        let on_next = cx.listener(|this, _, _, cx| {
            cx.update_entity(&this.viewer, |viewer, _| viewer.next_page());
            cx.notify();
        });
        let on_prev = cx.listener(|this, _, _, cx| {
            cx.update_entity(&this.viewer, |viewer, _| viewer.prev_page());
            cx.notify();
        });
        let on_open = cx.listener(|_, _, _, cx| {
            let wait = cx.prompt_for_paths(gpui::PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
            });
            cx.spawn(async move |my_app, mut app| {
                if let Ok(Ok(Some(path))) = wait.await {
                    if let Some(path) = path.get(0) {
                        if let Some(viewer) = my_app.upgrade() {
                            app.update_entity(&viewer, |my_app, cx| {
                                my_app.open(path, cx);
                            })
                            .unwrap();
                        }
                    }
                }
            })
            .detach();
        });

        let toolbox = cx.new({
            |cx| {
                Toolbox::new()
                    .child(cx.new(|_| ToolboxButton::new("Open...").on_click(on_open)))
                    .child(cx.new(|_| ToolboxButton::new("Next").on_click(on_next)))
                    .child(cx.new(|_| ToolboxButton::new("Prev").on_click(on_prev)))
            }
        });

        Self { toolbox, viewer }
    }

    fn open(&mut self, path: impl AsRef<Path>, cx: &mut Context<'_, Self>) {
        cx.update_entity(&self.viewer, |viewer, cx| {
            viewer.open(path);
            cx.notify();
        });
    }
}

impl Render for Viewer {
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
            .child(self.viewer.clone())
    }
}

struct MyApp {
    current: Either<Entity<Viewer>, Entity<Welcome>>,
}

impl MyApp {
    fn new(cx: &mut Context<'_, Self>) -> Self {
        let on_open = cx.listener(Self::on_open);
        Self {
            current: Either::Right(cx.new(|_| Welcome::new(on_open))),
        }
    }

    fn open(&mut self, cx: &mut Context<'_, Self>) {
        let wait = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
        });
        cx.spawn(async move |my_app, mut app| {
            if let Ok(Ok(Some(path))) = wait.await {
                if let Some(path) = path.get(0) {
                    if let Some(viewer) = my_app.upgrade() {
                        app.update_entity(&viewer, |my_app, cx| {
                            let viewer = cx.new(|cx| Viewer::new(cx, path));
                            my_app.current = Either::Left(viewer);
                            cx.notify();
                        })
                        .unwrap();
                    }
                }
            }
        })
        .detach();
    }

    fn on_open(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<'_, Self>) {
        info!("on open");
        let wait = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
        });
        cx.spawn(async move |my_app, mut app| {
            if let Ok(Ok(Some(path))) = wait.await {
                if let Some(path) = path.get(0) {
                    if let Some(viewer) = my_app.upgrade() {
                        app.update_entity(&viewer, |my_app, cx| {
                            let viewer = cx.new(|cx| Viewer::new(cx, path));
                            my_app.current = Either::Left(viewer);
                            cx.notify();
                        })
                        .unwrap();
                    }
                }
            }
        })
        .detach();
    }
}

impl Render for MyApp {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<'_, Self>) -> impl IntoElement {
        match &self.current {
            Either::Left(viewer) => viewer.clone().into_any_element(),
            Either::Right(welcome) => welcome.clone().into_any_element(),
        }
    }
}

actions!(Self, [Quit, Open, Next, Prev]);

fn main() {
    colog::init();

    Application::new().run(|cx| {
        settings::init(cx);
        SystemAppearance::init(cx);
        theme::init(LoadThemes::JustBase, cx);

        cx.on_action(|_: &Quit, cx| cx.quit());
        // cx.on_action(|_: &Open, cx| info!("on open in app"));
        cx.bind_keys([
            KeyBinding::new("ctrl-o", Open, None),
            KeyBinding::new("ctrl-q", Quit, None),
        ]);
        cx.set_menus(vec![Menu {
            name: "File".into(),
            items: vec![
                MenuItem::action("Open", Open),
                MenuItem::action("Quit", Quit),
            ],
        }]);
        let w = cx
            .open_window(
                WindowOptions {
                    titlebar: Some(TitlebarOptions {
                        title: Some("Nipdf Reader".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |_, cx| cx.new(MyApp::new),
            )
            .unwrap();
        let my_app = w.update(cx, |_, _, cx| cx.entity()).unwrap();
        cx.on_action(move |_: &Open, cx| {
            cx.update_entity(&my_app, |my_app, cx| my_app.open(cx));
        });
    });
}
