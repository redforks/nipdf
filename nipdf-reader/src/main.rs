use std::sync::Arc;

use anyhow::Result;
use iced::widget::{
    button, column,
    image::{Handle, Image},
    row, Text,
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

struct Page {
    width: u32,
    height: u32,
    data: ShardedData,
}

struct App {
    file_path: String,
    page: Option<Page>,
    err: Option<anyhow::Error>,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Stub,
}

impl App {
    /// load pdf file at `file_path` using `nipdf`, render page `no` to image and save to
    /// `self.page`
    #[save_error]
    fn load_page(&mut self, no: usize) -> Result<()> {
        let buf: Vec<u8> = std::fs::read(&self.file_path)?;
        let (f, resolver) = PdfFile::parse(&buf[..])?;
        let catalog = f.catalog(&resolver)?;
        let page = &catalog.pages()?[no];
        let option = RenderOptionBuilder::new().zoom(1.75);
        let page = page.render(option)?;
        let page = Page {
            width: page.width(),
            height: page.height(),
            data: ShardedData(Arc::new(page.take())),
        };
        self.page = Some(page);
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
        };
        r.load_page(0);
        r
    }

    fn title(&self) -> String {
        String::from(format!("nipdf - {}", self.file_path))
    }

    fn update(&mut self, message: Message) {}

    fn view(&self) -> Element<Message> {
        // show self.err if it is Some
        if let Some(err) = &self.err {
            return Text::new(format!("{}", err)).into();
        }

        column![
            row![
                button("Prev").on_press(Message::Stub),
                button("Next").on_press(Message::Stub),
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
