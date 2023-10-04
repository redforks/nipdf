use iced::widget::{button, column, text, Image};
use iced::{Alignment, Element, Sandbox, Settings};

fn main() -> iced::Result {
    App::run(Settings::default())
}

struct App {
    file_path: String,
}

#[derive(Debug, Clone, Copy)]
enum Message {}

impl Sandbox for App {
    type Message = Message;

    fn new() -> Self {
        Self {
            file_path: "/tmp/foo.png".to_owned(),
        }
    }

    fn title(&self) -> String {
        String::from(format!("nipdf - {}", self.file_path))
    }

    fn update(&mut self, message: Message) {}

    fn view(&self) -> Element<Message> {
        Image::new(&self.file_path).into()
    }
}
