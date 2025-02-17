use crate::{AppMessage, Result};
use iced::{
    Element, Length,
    widget::{
        button, column, horizontal_space,
        image::{Handle, Image},
        row, scrollable,
        scrollable::Direction,
        text, text_input,
    },
};
use nipdf::file::File as PdfFile;
use nipdf_render::{RenderOptionBuilder, render_page};
use snafu::ResultExt;

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

    pub fn can_next(self) -> bool {
        (self.current_page + 1) < self.total_pages
    }

    pub fn can_prev(self) -> bool {
        self.current_page > 0
    }
}

/// Current displayed Pdf rendered page.
struct Page {
    width: u32,
    height: u32,
    data: Vec<u8>,
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
}

/// Pdf file viewer
pub struct Viewer {
    page: Page,
    navi: PageNavigator,
    zoom: f32,
    cur_page_editing: String,
    file: PdfFile,
}

impl Viewer {
    pub fn new(file_path: impl Into<String>, password: impl Into<String>) -> Result<Self> {
        let file_path = file_path.into();
        let password = password.into();
        let file_data = std::fs::read(&file_path).whatever_context("read pdf file content")?;
        let file = PdfFile::parse(file_data, &password).whatever_context("parse pdf file")?;
        let mut r = Self {
            page: Page {
                width: 0,
                height: 0,
                data: vec![],
            },
            navi: PageNavigator {
                current_page: 0,
                total_pages: 0,
            },
            zoom: 1.75,
            cur_page_editing: "".to_owned(),
            file,
        };
        r.load_page(0)?;
        Ok(r)
    }

    fn update_cur_page_editing_from_navigation(&mut self) {
        self.cur_page_editing = format!("{}", self.navi.current_page + 1);
    }

    fn load_page(&mut self, no: u32) -> Result<()> {
        let resolver = self.file.resolver().whatever_context("parse resolver")?;
        let catalog = self
            .file
            .catalog(&resolver)
            .whatever_context("parse catalog")?;
        let pages = catalog.pages().whatever_context("parse page tree")?;
        let page = &pages[no as usize];
        let option = RenderOptionBuilder::new().zoom(self.zoom);
        let image = render_page(page, option).whatever_context("render page")?;
        self.page = Page {
            width: image.width(),
            height: image.height(),
            data: image.into_vec(),
        };
        self.navi = PageNavigator {
            current_page: no,
            total_pages: pages
                .len()
                .try_into()
                .whatever_context("pages out of range")?,
        };
        self.update_cur_page_editing_from_navigation();
        Ok(())
    }

    pub fn update(&mut self, message: ViewerMessage) -> Result<()> {
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
        }
    }

    pub(crate) fn view(&self) -> Element<'_, AppMessage> {
        let main: Element<'_, AppMessage> = column![
            row![
                // can not use row! macro, it has compile problems because of #[cfg] attribute on
                // some of items
                button("Open...").on_press(AppMessage::SelectFile),
                horizontal_space().width(16),
                text_input("Page", &self.cur_page_editing)
                    .width(60)
                    .on_input(|s| AppMessage::Viewer(ViewerMessage::CurPageChange(s)))
                    .on_submit(AppMessage::Viewer(ViewerMessage::CurPageChanged)),
                text(format!("/{}", self.navi.total_pages)),
                horizontal_space().width(16),
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
                horizontal_space().width(16),
                button("Zoom In").on_press(AppMessage::Viewer(ViewerMessage::ZoomIn)),
                button("Zoom Out").on_press(AppMessage::Viewer(ViewerMessage::ZoomOut)),
                horizontal_space().width(Length::Fill),
            ]
            .align_y(iced::Alignment::Center),
            scrollable(
                Image::new(Handle::from_rgba(
                    self.page.width,
                    self.page.height,
                    self.page.data.clone()
                ))
                .content_fit(iced::ContentFit::None)
            )
            .direction(Direction::Both {
                horizontal: Default::default(),
                vertical: Default::default()
            })
        ]
        .into();

        main
    }
}
