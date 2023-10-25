#[cfg(feature = "debug")]
use iced::{alignment, widget::horizontal_rule};
use std::sync::Arc;
#[cfg(feature = "debug")]
use std::time::{Duration, Instant};

use crate::{AppMessage, ShardedData};
use anyhow::Result;
use iced::{
    widget::{
        button, column, horizontal_space,
        image::{Handle, Image},
        row, scrollable,
        scrollable::{Direction, Properties},
        text, text_input,
    },
    Length,
};
use iced::{Color, Element};
#[cfg(feature = "debug")]
use iced_aw::{
    menu_bar,
    native::helpers::menu_tree,
    native::menu::{ItemHeight, MenuTree},
};
use nipdf::file::{File as PdfFile, ObjectResolver, RenderOptionBuilder, XRefTable};

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
    DumpPageThree,
    #[cfg(feature = "debug")]
    DumpPageThreeWithoutGvim,
}

/// Pdf file viewer
pub struct Viewer {
    file_path: String,
    page: Page,
    navi: PageNavigator,
    zoom: f32,
    cur_page_editing: String,
    #[cfg(feature = "debug")]
    render_time: Duration,
    file_data: Vec<u8>,
    xref: XRefTable,
    file: PdfFile,
}

impl Viewer {
    pub fn new(file_path: impl Into<String>) -> Result<Self> {
        let file_path = file_path.into();
        let file_data = std::fs::read(&file_path)?;
        let (file, xref) = PdfFile::parse(&file_data[..])?;
        let mut r = Self {
            file_path,
            page: Page {
                width: 0,
                height: 0,
                data: ShardedData(Arc::new(vec![])),
            },
            navi: PageNavigator {
                current_page: 0,
                total_pages: 0,
            },
            zoom: 1.75,
            cur_page_editing: "".to_owned(),
            #[cfg(feature = "debug")]
            render_time: Duration::default(),
            xref,
            file,
            file_data,
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
        let resolver = ObjectResolver::new(&self.file_data, &self.xref);
        let catalog = self.file.catalog(&resolver)?;
        let pages = catalog.pages()?;
        let page = &pages[no as usize];
        let option = RenderOptionBuilder::new().zoom(self.zoom);
        let page = page.render(option)?;
        self.page = Page {
            width: page.width(),
            height: page.height(),
            data: ShardedData(Arc::new(page.take())),
        };
        self.navi = PageNavigator {
            current_page: no,
            total_pages: pages.len() as u32,
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
        let resolver = ObjectResolver::new(&self.file_data, &self.xref);
        let catalog = self.file.catalog(&resolver)?;
        let pages = catalog.pages()?;
        let page = &pages[self.navi.current_page as usize];
        Ok(page.id())
    }

    /// Decode page content stream, dump using Debug trait and save to `/tmp/page-content` file.
    #[cfg(feature = "debug")]
    fn dump_page_content(&self) -> Result<()> {
        use std::io::Write;
        let resolver = ObjectResolver::new(&self.file_data, &self.xref);
        let catalog = self.file.catalog(&resolver)?;
        let pages = catalog.pages()?;
        let page = &pages[self.navi.current_page as usize];
        let contents = page.content()?;
        let mut f = std::fs::File::create("/tmp/page-content")?;
        for op in contents.operations() {
            writeln!(f, "{:?}", op)?;
        }
        Ok(())
    }

    /// Save page and related object using Debug trait and save to `/tmp/page-object` file.
    #[cfg(feature = "debug")]
    fn dump_page_object(&self) -> Result<()> {
        use nipdf::object::Object;
        use std::collections::HashSet;
        use std::io::Write;
        use std::num::NonZeroU32;

        let resolver = ObjectResolver::new(&self.file_data, &self.xref);
        let catalog = self.file.catalog(&resolver)?;
        let pages = catalog.pages()?;
        let page = &pages[self.navi.current_page as usize];

        let id = NonZeroU32::new(page.id()).unwrap();
        let mut id_wait_scanned = vec![id];
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
                        Some(r.id().id())
                    } else {
                        None
                    }
                }));
            }
        }

        Ok(())
    }

    /// Decode current page content stream and save to `/tmp/page-stream` file.
    #[cfg(feature = "debug")]
    fn dump_page_stream(&self) -> Result<()> {
        use std::io::Write;
        let resolver = ObjectResolver::new(&self.file_data, &self.xref);
        let catalog = self.file.catalog(&resolver)?;
        let pages = catalog.pages()?;
        let page = &pages[self.navi.current_page as usize];
        let contents = page.content()?;
        let mut f = std::fs::File::create("/tmp/page-stream")?;
        for buf in contents.as_ref() {
            f.write_all(buf)?;
        }
        Ok(())
    }

    /// Dump current page content/object/stream, and then open them
    /// in `gvim`
    #[cfg(feature = "debug")]
    fn dump_page_three(&self, open_in_gvim: bool) -> Result<()> {
        self.dump_page_object()?;
        self.dump_page_content()?;
        self.dump_page_stream()?;

        if open_in_gvim {
            use std::process::Command;
            Command::new("gvim")
                .arg("-O")
                .arg("/tmp/page-object")
                .arg("/tmp/page-content")
                .arg("/tmp/page-stream")
                .arg("-c")
                .arg("tabe /tmp/log")
                .spawn()?;
        }
        Ok(())
    }

    /// Run `cargo run -p nipdf-dump -- page -f <current pdf file path> --png <current page no>`,
    /// redirect `stderr` to `/tmp/log` file, Set environment string: `RUST_LOG=debug RUST_BACKTRACE=1`
    /// Return non-empty string on error
    #[cfg(feature = "debug")]
    fn dump_page_render_log(&self) -> Result<String> {
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
            .arg(format!("{}", self.navi.current_page))
            .stderr(std::fs::File::create("/tmp/log")?)
            .stdout(std::fs::File::open("/dev/null")?)
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
                self.dump_page_content()?;
                notify("Page content dumped to /tmp/page-content")
            }
            #[cfg(feature = "debug")]
            ViewerMessage::DumpPageObject => {
                self.dump_page_object()?;
                notify("Page object dumped to /tmp/page-object")
            }
            #[cfg(feature = "debug")]
            ViewerMessage::DumpPageStream => {
                self.dump_page_stream()?;
                notify("Page stream dumped to /tmp/page-stream")
            }
            #[cfg(feature = "debug")]
            ViewerMessage::DumpPageThree => self.dump_page_three(true),
            #[cfg(feature = "debug")]
            ViewerMessage::DumpPageThreeWithoutGvim => self.dump_page_three(false),
            #[cfg(feature = "debug")]
            ViewerMessage::DumpPageRenderLog => {
                let err = self.dump_page_render_log()?;
                if err.is_empty() {
                    notify("Page render log dumped to /tmp/log")
                } else {
                    notify(&format!("Error: {}", err))
                }
            }
        }
    }

    pub(crate) fn view(&self) -> Element<AppMessage> {
        column![
            row(vec![
                // can not use row! macro, it has compile problems because of #[cfg] attribute on some of items
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
                        new_menu_item("Dump Page and open", ViewerMessage::DumpPageThree),
                        new_menu_item("Dump Page", ViewerMessage::DumpPageThreeWithoutGvim),
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
        .into()
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
