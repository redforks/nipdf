use std::sync::Arc;
#[cfg(debug_assertions)]
use std::time::{Duration, Instant};

use crate::{AppMessage, ShardedData};
use anyhow::Result;
use iced::Element;
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
}

/// Pdf file viewer
pub struct Viewer {
    file_path: String,
    page: Page,
    navi: PageNavigator,
    zoom: f32,
    cur_page_editing: String,
    #[cfg(debug_assertions)]
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
            #[cfg(debug_assertions)]
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
        #[cfg(debug_assertions)]
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
        #[cfg(debug_assertions)]
        {
            self.render_time = now.elapsed();
        }
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
                #[cfg(debug_assertions)]
                text(format!("{} ms", self.render_time.as_millis())).into()
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
