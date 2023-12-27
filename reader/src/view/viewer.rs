use crate::{AppMessage, ShardedData};
use anyhow::Result;
#[cfg(feature = "debug")]
use iced::alignment::Horizontal;
#[cfg(feature = "debug")]
use iced::widget::{Button, Row, Text};
#[cfg(feature = "debug")]
use iced::{alignment, widget::horizontal_rule};
use iced::{
    widget::{
        button, column, horizontal_space,
        image::{Handle, Image},
        row, scrollable,
        scrollable::{Direction, Properties},
        text, text_input,
    },
    Color, Element, Length,
};
#[cfg(feature = "debug")]
use iced_aw::{
    menu_bar,
    native::helpers::menu_tree,
    native::menu::{ItemHeight, MenuTree},
};
#[cfg(feature = "debug")]
use iced_aw::{modal, Card};
use nipdf::file::File as PdfFile;
use nipdf_render::{render_page, RenderOptionBuilder};
#[cfg(feature = "debug")]
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Copy)]
struct PageNavigator {
    current_page: u32,
    total_pages: u32,
}

impl PageNavigator {
    pub fn next(&mut self) {
        if (self.current_page + 1) < self.total_pages {
            self.current_page += 1;
        }
    }

    pub fn prev(&mut self) {
        if self.current_page > 0 {
            self.current_page -= 1;
        }
    }

    pub fn can_next(&self) -> bool {
        (self.current_page + 1) < self.total_pages
    }

    pub fn can_prev(&self) -> bool {
        self.current_page > 0
    }
}

/// Current displayed Pdf rendered page.
struct Page {
    width: u32,
    height: u32,
    data: ShardedData,
}

#[cfg(feature = "debug")]
#[derive(Debug, Clone)]
pub enum PageInputMessage {
    Hide,
    EditValue(String),
}

#[cfg(feature = "debug")]
#[derive(Default)]
struct PageInput {
    visible: bool,
    place_holder: String,
    value: String,
}

#[cfg(feature = "debug")]
impl PageInput {
    pub fn update(&mut self, m: PageInputMessage) -> Result<()> {
        match m {
            PageInputMessage::Hide => {
                self.visible = false;
            }
            PageInputMessage::EditValue(s) => {
                self.value = s;
            }
        }

        Ok(())
    }

    pub fn view(&self) -> Element<AppMessage> {
        Card::new(
            Text::new(crate::APP_NAME),
            text_input(&self.place_holder, &self.value)
                .on_input(|v| {
                    AppMessage::Viewer(ViewerMessage::PageInput(PageInputMessage::EditValue(v)))
                })
                .on_submit(AppMessage::Viewer(ViewerMessage::DumpFourForSpecificPage)),
        )
        .foot(
            Row::new()
                .spacing(10)
                .padding(5)
                .width(Length::Fill)
                .push(
                    Button::new(Text::new("Cancel").horizontal_alignment(Horizontal::Center))
                        .width(Length::Fill)
                        .on_press(AppMessage::Viewer(ViewerMessage::PageInput(
                            PageInputMessage::Hide,
                        ))),
                )
                .push(
                    Button::new(Text::new("Ok").horizontal_alignment(Horizontal::Center))
                        .width(Length::Fill)
                        .on_press(AppMessage::Viewer(ViewerMessage::DumpFourForSpecificPage)),
                ),
        )
        .max_width(300.0)
        .on_close(AppMessage::CancelSelectFile)
        .into()
    }
}

/// Messages for pdf file viewer view.
#[derive(Debug, Clone)]
pub enum ViewerMessage {
    NextPage,
    PrevPage,
    ZoomIn,
    ZoomOut,

    CurPageChange(String),
    CurPageChanged,

    #[cfg(feature = "debug")]
    ShowPageId,
    #[cfg(feature = "debug")]
    DumpPageObject,
    #[cfg(feature = "debug")]
    DumpPageContent,
    #[cfg(feature = "debug")]
    DumpPageStream,
    #[cfg(feature = "debug")]
    DumpPageRenderLog,
    #[cfg(feature = "debug")]
    DumpPage,
    #[cfg(feature = "debug")]
    DumpPageWithoutGvim,
    #[cfg(feature = "debug")]
    DumpFourForSpecificPage,
    #[cfg(feature = "debug")]
    PageInput(PageInputMessage),
}

/// Pdf file viewer
pub struct Viewer {
    file_path: String,
    page: Page,
    navi: PageNavigator,
    zoom: f32,
    cur_page_editing: String,
    file: PdfFile,
    #[cfg(feature = "debug")]
    render_time: Duration,
    #[cfg(feature = "debug")]
    page_input: PageInput,
    #[cfg(feature = "debug")]
    open_in_gvim: bool,
}

impl Viewer {
    pub fn new(file_path: impl Into<String>, password: impl Into<String>) -> Result<Self> {
        let file_path = file_path.into();
        let password = password.into();
        let file_data = std::fs::read(&file_path)?;
        let file = PdfFile::parse(file_data, &password)?;
        let mut r = Self {
            file_path,
            page: Page {
                width: 0,
                height: 0,
                data: ShardedData(vec![].into()),
            },
            navi: PageNavigator {
                current_page: 0,
                total_pages: 0,
            },
            zoom: 1.75,
            cur_page_editing: "".to_owned(),
            #[cfg(feature = "debug")]
            render_time: Duration::default(),
            file,
            #[cfg(feature = "debug")]
            page_input: PageInput::default(),
            #[cfg(feature = "debug")]
            open_in_gvim: false,
        };
        r.load_page(0)?;
        Ok(r)
    }

    pub fn file_path(&self) -> &str {
        &self.file_path
    }

    fn update_cur_page_editing_from_navigation(&mut self) {
        self.cur_page_editing = format!("{}", self.navi.current_page + 1);
    }

    fn load_page(&mut self, no: u32) -> Result<()> {
        #[cfg(feature = "debug")]
        let now = Instant::now();
        let resolver = self.file.resolver()?;
        let catalog = self.file.catalog(&resolver)?;
        let pages = catalog.pages()?;
        let page = &pages[no as usize];
        let option = RenderOptionBuilder::new().zoom(self.zoom);
        let image = render_page(page, option)?;
        self.page = Page {
            width: image.width(),
            height: image.height(),
            data: ShardedData(image.into_vec().into()),
        };
        self.navi = PageNavigator {
            current_page: no,
            total_pages: pages.len().try_into().unwrap(),
        };
        self.update_cur_page_editing_from_navigation();
        #[cfg(feature = "debug")]
        {
            self.render_time = now.elapsed();
        }
        Ok(())
    }

    #[cfg(feature = "debug")]
    fn page_object_number(&self) -> Result<u32> {
        let resolver = self.file.resolver()?;
        let catalog = self.file.catalog(&resolver)?;
        let pages = catalog.pages()?;
        let page = &pages[self.navi.current_page as usize];
        Ok(page.id().0)
    }

    /// Decode page content stream, dump using Debug trait and save to `/tmp/page-content` file.
    /// Use current page if page_no is None.
    #[cfg(feature = "debug")]
    fn dump_page_content(&self, page_no: Option<u32>) -> Result<()> {
        let page_no = page_no.unwrap_or_else(|| self.navi.current_page) as usize;

        use std::io::Write;
        let resolver = self.file.resolver()?;
        let catalog = self.file.catalog(&resolver)?;
        let pages = catalog.pages()?;
        let page = &pages[page_no];
        let contents = page.content()?;
        let mut f = std::fs::File::create("/tmp/page-content")?;
        for op in contents.operations() {
            writeln!(f, "{:?}", op)?;
        }
        Ok(())
    }

    /// Save page and related object using Debug trait and save to `/tmp/page-object` file.
    /// Use current page if page_no is None.
    #[cfg(feature = "debug")]
    fn dump_page_object(&self, page_no: Option<u32>) -> Result<()> {
        let page_no = page_no.unwrap_or_else(|| self.navi.current_page) as usize;

        use nipdf::object::Object;
        use std::{collections::HashSet, io::Write};

        let resolver = self.file.resolver()?;
        let catalog = self.file.catalog(&resolver)?;
        let pages = catalog.pages()?;
        let page = &pages[page_no];

        let mut id_wait_scanned = vec![page.id().0];
        let mut ids = HashSet::new();

        let mut f = std::fs::File::create("/tmp/page-object")?;
        while let Some(id) = id_wait_scanned.pop() {
            if ids.insert(id) {
                writeln!(&mut f, "OBJ {}:", id)?;
                let obj = resolver.resolve(id)?;
                obj.to_doc().render(80, &mut f)?;
                writeln!(&mut f, "\n\n\n")?;

                id_wait_scanned.extend(obj.iter_values().filter_map(|o| {
                    if let Object::Reference(r) = o {
                        Some(r.id().id().0)
                    } else {
                        None
                    }
                }));
            }
        }

        Ok(())
    }

    /// Decode page content stream and save to `/tmp/page-stream` file.
    /// Use current page if page_no is None.
    #[cfg(feature = "debug")]
    fn dump_page_stream(&self, page_no: Option<u32>) -> Result<()> {
        let page_no = page_no.unwrap_or_else(|| self.navi.current_page) as usize;

        use std::io::Write;
        let resolver = self.file.resolver()?;
        let catalog = self.file.catalog(&resolver)?;
        let pages = catalog.pages()?;
        let page = &pages[page_no];
        let contents = page.content()?;
        let mut f = std::fs::File::create("/tmp/page-stream")?;
        for buf in contents.as_ref() {
            f.write_all(buf)?;
        }
        Ok(())
    }

    #[cfg(feature = "debug")]
    fn open_in_gvim() -> Result<()> {
        use std::process::Command;
        Command::new("gvim")
            .arg("-O")
            .arg("/tmp/page-object")
            .arg("/tmp/page-content")
            .arg("/tmp/page-stream")
            .arg("-c")
            .arg("tabe /tmp/log")
            .spawn()?;
        Ok(())
    }

    /// Dump current page content/object/stream, and then open them
    /// in `gvim`
    #[cfg(feature = "debug")]
    fn dump_page(&mut self, open_in_gvim: bool) -> Result<()> {
        self.open_in_gvim = open_in_gvim;
        self.page_input.visible = true;
        self.page_input.place_holder = format!("{}", self.navi.current_page + 1);
        if self.page_input.value.is_empty() {
            self.page_input.value = self.page_input.place_holder.clone();
        }
        Ok(())
    }

    /// Run `cargo run -p nipdf-dump -- page -f <current pdf file path> --png <page no>`,
    /// redirect `stderr` to `/tmp/log` file, Set environment string: `RUST_LOG=debug
    /// RUST_BACKTRACE=1` Use current page if page_no is None
    /// Return non-empty string on error
    #[cfg(feature = "debug")]
    fn dump_page_render_log(&self, page_no: Option<u32>) -> Result<String> {
        let page_no = page_no.unwrap_or_else(|| self.navi.current_page) as usize;

        use std::process::Command;
        let child = Command::new("cargo")
            .arg("run")
            .arg("-p")
            .arg("nipdf-dump")
            .arg("--")
            .arg("page")
            .arg("-f")
            .arg(&self.file_path)
            .arg("--png")
            .arg(format!("{}", page_no))
            .stderr(std::fs::File::create("/tmp/log")?)
            .stdout(std::fs::File::create("/tmp/foo.png")?)
            .env("RUST_LOG", "debug")
            .env("RUST_BACKTRACE", "1")
            .spawn();
        Ok(match child {
            Err(err) => format!("{:?}", err),
            Ok(mut child) => child.wait().map_or_else(
                |e| format!("{:?}", e),
                |exit_code| {
                    if exit_code.success() {
                        format!("exit code: {}", exit_code)
                    } else {
                        "".to_owned()
                    }
                },
            ),
        })
    }

    /// Dump `page_no` page's content/object/stream, and render to `/tmp/foo.png`,
    /// save render log to `/tmp/log` file.
    #[cfg(feature = "debug")]
    fn dump_four_for_specific_page(&self, page_no: u32) -> Result<()> {
        self.dump_page_object(Some(page_no))?;
        self.dump_page_content(Some(page_no))?;
        self.dump_page_stream(Some(page_no))?;
        self.dump_page_render_log(Some(page_no))?;
        Ok(())
    }

    pub fn update(&mut self, message: ViewerMessage) -> Result<()> {
        #[cfg(feature = "debug")]
        fn notify(msg: &str) -> Result<()> {
            use notify_rust::Notification;
            Notification::new()
                .appname(crate::APP_NAME)
                .summary(msg)
                .show()?;
            Ok(())
        }

        match message {
            ViewerMessage::NextPage => {
                self.navi.next();
                self.load_page(self.navi.current_page)
            }
            ViewerMessage::PrevPage => {
                self.navi.prev();
                self.load_page(self.navi.current_page)
            }
            ViewerMessage::ZoomIn => {
                self.zoom *= 1.25;
                self.load_page(self.navi.current_page)
            }
            ViewerMessage::ZoomOut => {
                self.zoom /= 1.25;
                self.load_page(self.navi.current_page)
            }
            ViewerMessage::CurPageChange(s) => {
                self.cur_page_editing = s;
                Ok(())
            }
            ViewerMessage::CurPageChanged => {
                if let Ok(page) = self.cur_page_editing.parse::<u32>() {
                    if page > 0 && page <= self.navi.total_pages {
                        self.navi.current_page = page - 1;
                        self.load_page(self.navi.current_page)
                    } else {
                        self.update_cur_page_editing_from_navigation();
                        Ok(())
                    }
                } else {
                    self.update_cur_page_editing_from_navigation();
                    Ok(())
                }
            }
            #[cfg(feature = "debug")]
            ViewerMessage::ShowPageId => {
                let id = self.page_object_number()?;
                notify(&id.to_string())
            }
            #[cfg(feature = "debug")]
            ViewerMessage::DumpPageContent => {
                self.dump_page_content(None)?;
                notify("Page content dumped to /tmp/page-content")
            }
            #[cfg(feature = "debug")]
            ViewerMessage::DumpPageObject => {
                self.dump_page_object(None)?;
                notify("Page object dumped to /tmp/page-object")
            }
            #[cfg(feature = "debug")]
            ViewerMessage::DumpPageStream => {
                self.dump_page_stream(None)?;
                notify("Page stream dumped to /tmp/page-stream")
            }
            #[cfg(feature = "debug")]
            ViewerMessage::DumpPage => self.dump_page(true),
            #[cfg(feature = "debug")]
            ViewerMessage::DumpPageWithoutGvim => self.dump_page(false),
            #[cfg(feature = "debug")]
            ViewerMessage::DumpPageRenderLog => {
                let err = self.dump_page_render_log(None)?;
                if err.is_empty() {
                    notify("Page render log dumped to /tmp/log")
                } else {
                    notify(&format!("Error: {}", err))
                }
            }
            #[cfg(feature = "debug")]
            ViewerMessage::DumpFourForSpecificPage => {
                if let Ok(page) = self.page_input.value.parse::<u32>() {
                    self.page_input.visible = false;
                    let page = page.clamp(1, self.navi.total_pages);
                    self.dump_four_for_specific_page(page - 1)?;
                    if self.open_in_gvim {
                        Self::open_in_gvim()
                    } else {
                        notify("Dumped page content/object/stream/render-log for specific page")
                    }
                } else {
                    notify("Invalid page number")
                }
            }
            #[cfg(feature = "debug")]
            ViewerMessage::PageInput(m) => self.page_input.update(m),
        }
    }

    pub(crate) fn view(&self) -> Element<AppMessage> {
        let main: Element<AppMessage> = column![
            row(vec![
                // can not use row! macro, it has compile problems because of #[cfg] attribute on
                // some of items
                button("Open...").on_press(AppMessage::SelectFile).into(),
                horizontal_space(16).into(),
                text_input("Page", &self.cur_page_editing)
                    .width(60)
                    .on_input(|s| AppMessage::Viewer(ViewerMessage::CurPageChange(s)))
                    .on_submit(AppMessage::Viewer(ViewerMessage::CurPageChanged))
                    .into(),
                text(format!("/{}", self.navi.total_pages)).into(),
                horizontal_space(16).into(),
                button("Prev")
                    .on_press_maybe(
                        self.navi
                            .can_prev()
                            .then_some(AppMessage::Viewer(ViewerMessage::PrevPage))
                    )
                    .into(),
                button("Next")
                    .on_press_maybe(
                        self.navi
                            .can_next()
                            .then_some(AppMessage::Viewer(ViewerMessage::NextPage))
                    )
                    .into(),
                horizontal_space(16).into(),
                button("Zoom In")
                    .on_press(AppMessage::Viewer(ViewerMessage::ZoomIn))
                    .into(),
                button("Zoom Out")
                    .on_press(AppMessage::Viewer(ViewerMessage::ZoomOut))
                    .into(),
                horizontal_space(Length::Fill).into(),
                #[cfg(feature = "debug")]
                text(format!("{} ms", self.render_time.as_millis())).into(),
                #[cfg(feature = "debug")]
                horizontal_space(8).into(),
                #[cfg(feature = "debug")]
                menu_bar!(menu_tree(
                    text("Debug"),
                    vec![
                        new_menu_item("Page Object id", ViewerMessage::ShowPageId),
                        MenuTree::new(horizontal_rule(4)),
                        new_menu_item("Page Object", ViewerMessage::DumpPageObject),
                        new_menu_item("Page Content", ViewerMessage::DumpPageContent),
                        new_menu_item("Page Stream", ViewerMessage::DumpPageStream),
                        new_menu_item("Page Render Log", ViewerMessage::DumpPageRenderLog),
                        MenuTree::new(horizontal_rule(4)),
                        new_menu_item("Dump Page and open", ViewerMessage::DumpPage),
                        new_menu_item("Dump Page", ViewerMessage::DumpPageWithoutGvim),
                    ]
                ))
                .item_height(ItemHeight::Dynamic(24))
                .into()
            ])
            .align_items(iced::Alignment::Center),
            scrollable(
                Image::new(Handle::from_pixels(
                    self.page.width,
                    self.page.height,
                    self.page.data.clone(),
                ))
                .content_fit(iced::ContentFit::None)
            )
            .direction(Direction::Both {
                vertical: Properties::default(),
                horizontal: Properties::default(),
            })
        ]
        .into();

        if cfg!(feature = "debug") {
            #[cfg(feature = "debug")]
            return modal(
                main,
                self.page_input.visible.then(|| self.page_input.view()),
            )
            .into();
        }

        main
    }
}

#[cfg(feature = "debug")]
fn new_menu_item(label: &str, message: ViewerMessage) -> MenuTree<AppMessage, iced::Renderer> {
    MenuTree::new(
        button(
            text(label)
                .width(Length::Fill)
                .height(Length::Fill)
                .vertical_alignment(alignment::Vertical::Center),
        )
        .padding([4, 8])
        .style(iced::theme::Button::Custom(Box::new(ButtonStyle {})))
        .on_press(AppMessage::Viewer(message)),
    )
}

struct ButtonStyle;
impl button::StyleSheet for ButtonStyle {
    type Style = iced::Theme;

    fn active(&self, style: &Self::Style) -> button::Appearance {
        button::Appearance {
            text_color: style.extended_palette().background.base.text,
            border_radius: [4.0; 4].into(),
            background: Some(Color::TRANSPARENT.into()),
            ..Default::default()
        }
    }

    fn hovered(&self, style: &Self::Style) -> button::Appearance {
        let plt = style.extended_palette();

        button::Appearance {
            background: Some(plt.primary.weak.color.into()),
            text_color: plt.primary.weak.text,
            ..self.active(style)
        }
    }
}
