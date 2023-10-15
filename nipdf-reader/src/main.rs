use std::sync::Arc;

use anyhow::Result;
use iced::{
    alignment::Horizontal,
    widget::{
        button, column, horizontal_space,
        image::{Handle, Image},
        row, scrollable,
        scrollable::{Direction, Properties},
        text, text_input, Button, Row, Text,
    },
    Length,
};
use iced::{Element, Sandbox, Settings};
use iced_aw::{modal, Card};
use nipdf::file::{File as PdfFile, RenderOptionBuilder};
use nipdf_macro::save_error;

fn main() -> iced::Result {
    env_logger::init();

    App::run(Settings::default())
}

/// Share [u8] data, implements `AsRef<[u8]` trait, `Arc<Vec<u8>>` itself not implement the trait.
#[derive(Clone)]
struct ShardedData(Arc<Vec<u8>>);

impl AsRef<[u8]> for ShardedData {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

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

struct Page {
    width: u32,
    height: u32,
    data: ShardedData,
}

struct App {
    file_path: String,
    page: Option<Page>,
    err: Option<anyhow::Error>,
    navi: PageNavigator,
    zoom: f32,
    selecting_file: bool,
    file_path_selecting: String,
    cur_page_editing: String,
}

/// Messages for pdf file viewer view.
#[derive(Debug, Clone)]
enum ViewerMessage {
    NextPage,
    PrevPage,
    ZoomIn,
    ZoomOut,

    CurPageChange(String),
    CurPageChanged,
}

/// Messages for application view.
#[derive(Debug, Clone)]
enum AppMessage {
    Viewer(ViewerMessage),

    SelectFile,
    SelectedFileChange(String),
    CancelSelectFile,
    FileSelected(String),
}

impl App {
    /// load pdf file at `file_path` using `nipdf`, render page `no` to image and save to
    /// `self.page`
    #[save_error]
    fn load_page(&mut self, no: u32) -> Result<()> {
        let buf: Vec<u8> = std::fs::read(&self.file_path)?;
        let (f, resolver) = PdfFile::parse(&buf[..])?;
        let catalog = f.catalog(&resolver)?;
        let pages = catalog.pages()?;
        let page = &pages[no as usize];
        let option = RenderOptionBuilder::new().zoom(self.zoom);
        let page = page.render(option)?;
        let page = Page {
            width: page.width(),
            height: page.height(),
            data: ShardedData(Arc::new(page.take())),
        };
        self.page = Some(page);
        self.navi = PageNavigator {
            current_page: no,
            total_pages: pages.len() as u32,
        };
        self.update_cur_page_editing_from_navigation();
        Ok(())
    }

    fn file_modal_view(&self) -> Element<'_, AppMessage> {
        Card::new(
            Text::new("nipdf"),
            text_input("pdf file path", &self.file_path_selecting)
                .on_input(AppMessage::SelectedFileChange),
        )
        .foot(
            Row::new()
                .spacing(10)
                .padding(5)
                .width(Length::Fill)
                .push(
                    Button::new(Text::new("Cancel").horizontal_alignment(Horizontal::Center))
                        .width(Length::Fill)
                        .on_press(AppMessage::CancelSelectFile),
                )
                .push(
                    Button::new(Text::new("Ok").horizontal_alignment(Horizontal::Center))
                        .width(Length::Fill)
                        .on_press(AppMessage::FileSelected(self.file_path_selecting.clone())),
                ),
        )
        .max_width(300.0)
        //.width(Length::Shrink)
        .on_close(AppMessage::CancelSelectFile)
        .into()
    }

    fn error(&mut self, e: anyhow::Error) {
        self.err = Some(e);
        self.selecting_file = false;
    }

    fn update_cur_page_editing_from_navigation(&mut self) {
        self.cur_page_editing = format!("{}", self.navi.current_page + 1);
    }

    fn viewer_update(&mut self, message: ViewerMessage) {
        match message {
            ViewerMessage::NextPage => {
                self.navi.next();
                self.load_page(self.navi.current_page);
            }
            ViewerMessage::PrevPage => {
                self.navi.prev();
                self.load_page(self.navi.current_page);
            }
            ViewerMessage::ZoomIn => {
                self.zoom *= 1.25;
                self.load_page(self.navi.current_page);
            }
            ViewerMessage::ZoomOut => {
                self.zoom /= 1.25;
                self.load_page(self.navi.current_page);
            }
            ViewerMessage::CurPageChange(s) => {
                self.cur_page_editing = s;
            }
            ViewerMessage::CurPageChanged => {
                if let Ok(page) = self.cur_page_editing.parse::<u32>() {
                    if page > 0 && page <= self.navi.total_pages {
                        self.navi.current_page = page - 1;
                        self.load_page(self.navi.current_page);
                    } else {
                        self.update_cur_page_editing_from_navigation();
                    }
                } else {
                    self.update_cur_page_editing_from_navigation();
                }
            }
        }
    }
}

impl Sandbox for App {
    type Message = AppMessage;

    fn new() -> Self {
        let mut r = Self {
            file_path: "/tmp/pdfreference1.0.pdf".to_owned(),
            page: None,
            err: None,
            navi: PageNavigator {
                current_page: 0,
                total_pages: 0,
            },
            zoom: 1.75,
            selecting_file: false,
            file_path_selecting: "".to_owned(),
            cur_page_editing: "".to_owned(),
        };
        r.load_page(0);
        r
    }

    fn title(&self) -> String {
        format!("nipdf - {}", self.file_path)
    }

    fn update(&mut self, message: AppMessage) {
        match message {
            AppMessage::Viewer(msg) => self.viewer_update(msg),
            AppMessage::SelectFile => {
                self.selecting_file = true;
                self.file_path_selecting = self.file_path.clone();
            }
            AppMessage::SelectedFileChange(path) => {
                self.file_path_selecting = path;
            }
            AppMessage::CancelSelectFile => {
                self.selecting_file = false;
            }
            AppMessage::FileSelected(path) => {
                self.file_path = path;
                self.selecting_file = false;
                self.load_page(0);
            }
        }
    }

    fn view(&self) -> Element<AppMessage> {
        // show self.err if it is Some
        let main = if let Some(err) = &self.err {
            row![
                Text::new(format!("{}", err)),
                button("Open a new file...").on_press(AppMessage::SelectFile),
            ]
            .into()
        } else {
            column![
                row![
                    button("Open...").on_press(AppMessage::SelectFile),
                    horizontal_space(16),
                    text_input("Page", &self.cur_page_editing)
                        .width(60)
                        .on_input(|s| AppMessage::Viewer(ViewerMessage::CurPageChange(s)))
                        .on_submit(AppMessage::Viewer(ViewerMessage::CurPageChanged)),
                    text(format!(
                        "{}/{}",
                        self.navi.current_page + 1,
                        self.navi.total_pages
                    )),
                    horizontal_space(16),
                    button("Prev").on_press_maybe(
                        self.navi
                            .can_prev()
                            .then_some(AppMessage::Viewer(ViewerMessage::PrevPage))
                    ),
                    button("Next").on_press_maybe(
                        self.navi
                            .can_next()
                            .then_some(AppMessage::Viewer(ViewerMessage::NextPage))
                    ),
                    horizontal_space(16),
                    button("Zoom In").on_press(AppMessage::Viewer(ViewerMessage::ZoomIn)),
                    button("Zoom Out").on_press(AppMessage::Viewer(ViewerMessage::ZoomOut)),
                ]
                .align_items(iced::Alignment::Center),
                match &self.page {
                    Some(page) => Element::from(
                        scrollable(
                            Image::new(Handle::from_pixels(
                                page.width,
                                page.height,
                                page.data.clone(),
                            ))
                            .content_fit(iced::ContentFit::None)
                        )
                        .direction(Direction::Both {
                            vertical: Properties::default(),
                            horizontal: Properties::default(),
                        })
                    ),
                    None => Text::new("No page").into(),
                }
            ]
            .into()
        };

        if self.selecting_file {
            modal(main, Some(self.file_modal_view())).into()
        } else {
            main
        }
    }
}
