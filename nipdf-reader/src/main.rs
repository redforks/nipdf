use std::sync::Arc;

use anyhow::Result;
use iced::widget::{
    button, column,
    image::{Handle, Image},
    row, Text, horizontal_space,
};
use iced::{Element, Sandbox, Settings};
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
}

#[derive(Debug, Clone, Copy)]
enum Message {
    NextPage,
    PrevPage,
    ZoomIn,
    ZoomOut,
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
        Ok(())
    }
}

impl Sandbox for App {
    type Message = Message;

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
        };
        r.load_page(0);
        r
    }

    fn title(&self) -> String {
        String::from(format!("nipdf - {}", self.file_path))
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::NextPage => {
                self.navi.next();
                self.load_page(self.navi.current_page);
            }
            Message::PrevPage => {
                self.navi.prev();
                self.load_page(self.navi.current_page);
            }
            Message::ZoomIn => {
                self.zoom *= 1.25;
                self.load_page(self.navi.current_page);
            }
            Message::ZoomOut => {
                self.zoom /= 1.25;
                self.load_page(self.navi.current_page);
            }
        }
    }

    fn view(&self) -> Element<Message> {
        // show self.err if it is Some
        if let Some(err) = &self.err {
            return Text::new(format!("{}", err)).into();
        }

        column![
            row![
                button("Prev").on_press_maybe(self.navi.can_prev().then_some(Message::PrevPage)),
                button("Next").on_press_maybe(self.navi.can_next().then_some(Message::NextPage)),
                horizontal_space(16),
                button("Zoom In").on_press(Message::ZoomIn),
                button("Zoom Out").on_press(Message::ZoomOut),
            ],
            match &self.page {
                Some(page) => Element::from(Image::new(Handle::from_pixels(
                    page.width,
                    page.height,
                    page.data.clone(),
                ))),
                None => Text::new("No page").into(),
            }
        ]
        .into()
    }
}
