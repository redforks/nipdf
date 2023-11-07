use std::sync::Arc;

use anyhow::Result;
use iced::{
    alignment::Horizontal,
    executor, font,
    widget::{text_input, Button, Row, Text},
    Application, Command, Length, Theme,
};
use iced::{Element, Settings};
use iced_aw::{modal, Card};

mod app_state;
mod view;
use log::error;
use std::env;
use view::{
    error::ErrorView,
    viewer::{Viewer, ViewerMessage},
    welcome::Welcome,
};

const APP_NAME: &str = "nipdf";

fn main() -> iced::Result {
    env_logger::init();

    let mut args: Vec<String> = env::args().collect();
    let filename = (args.len() >= 2).then(|| args.remove(1));

    App::run(Settings::with_flags(filename))
}

/// Share [u8] data, implements `AsRef<[u8]` trait, `Arc<Vec<u8>>` itself not implement the trait.
#[derive(Clone)]
struct ShardedData(Arc<[u8]>);

impl AsRef<[u8]> for ShardedData {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

enum View {
    Error(ErrorView),
    Viewer(Box<Viewer>),
    Welcome(Welcome),
}

/// Messages for application view.
#[derive(Debug, Clone)]
enum AppMessage {
    Initialized,
    Viewer(ViewerMessage),

    SelectFile,
    SelectedFileChange(String),
    CancelSelectFile,
    FileSelected,
}

struct App {
    current: View,
    selecting_file: bool,
    file_path_selecting: String,
}

impl App {
    fn viewer(&self) -> Option<&Viewer> {
        match self.current {
            View::Viewer(ref v) => Some(v),
            _ => None,
        }
    }

    fn mut_viewer(&mut self) -> Option<&mut Viewer> {
        match self.current {
            View::Viewer(ref mut v) => Some(v),
            _ => None,
        }
    }

    fn file_modal_view(&self) -> Element<'_, AppMessage> {
        Card::new(
            Text::new(APP_NAME),
            text_input("pdf file path", &self.file_path_selecting)
                .on_input(AppMessage::SelectedFileChange)
                .on_submit(AppMessage::FileSelected),
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
                        .on_press(AppMessage::FileSelected),
                ),
        )
        .max_width(300.0)
        .on_close(AppMessage::CancelSelectFile)
        .into()
    }

    fn handle_result<T>(&mut self, rv: Result<T>) -> Option<T> {
        match rv {
            Ok(v) => Some(v),
            Err(e) => {
                self.current = View::Error(ErrorView::new(e));
                self.selecting_file = false;
                None
            }
        }
    }

    fn open_last_file(&mut self) {
        if let Some(p) = app_state::load_last_file() {
            match Viewer::new(p) {
                Ok(v) => {
                    self.current = View::Viewer(Box::new(v));
                }
                Err(e) => {
                    error!("open last file failed: {}", e);
                }
            }
        }
    }

    fn open(&mut self) {
        let file_path = self.file_path_selecting.clone();
        if let Some(viewer) = self.handle_result(Viewer::new(file_path)) {
            self.current = View::Viewer(Box::new(viewer));
            app_state::save_last_file(&self.file_path_selecting);
        }
    }
}

impl Application for App {
    type Message = AppMessage;
    type Flags = Option<String>;
    type Executor = executor::Default;
    type Theme = Theme;

    fn new(file_path: Self::Flags) -> (Self, Command<Self::Message>) {
        let mut r = Self {
            current: View::Welcome(Welcome),
            selecting_file: false,
            file_path_selecting: "".to_owned(),
        };
        if let Some(path) = file_path {
            r.file_path_selecting = path;
            r.open();
        } else {
            r.open_last_file();
        }
        (
            r,
            // load icon font for iced_aw, without this modal close button icon will not show.
            font::load(iced_aw::graphics::icons::ICON_FONT_BYTES).map(|_| AppMessage::Initialized),
        )
    }

    fn title(&self) -> String {
        self.viewer().map_or(APP_NAME.to_owned(), |v| {
            format!("{APP_NAME} - {}", v.file_path())
        })
    }

    fn update(&mut self, message: AppMessage) -> Command<Self::Message> {
        match message {
            AppMessage::Initialized => {}
            AppMessage::Viewer(msg) => {
                let rv = self.mut_viewer().unwrap().update(msg);
                self.handle_result(rv);
            }

            AppMessage::SelectFile => {
                self.selecting_file = true;
                if let Some(viewer) = self.viewer() {
                    self.file_path_selecting = viewer.file_path().to_owned();
                }
            }
            AppMessage::SelectedFileChange(path) => {
                self.file_path_selecting = path;
            }
            AppMessage::CancelSelectFile => {
                self.selecting_file = false;
            }
            AppMessage::FileSelected => {
                self.open();
                self.selecting_file = false;
            }
        }

        Command::none()
    }

    fn view(&self) -> Element<AppMessage> {
        let main = match &self.current {
            View::Viewer(v) => v.view(),
            View::Error(v) => v.view(),
            View::Welcome(v) => v.view(),
        };

        if self.selecting_file {
            modal(main, Some(self.file_modal_view()))
                .on_esc(AppMessage::CancelSelectFile)
                .backdrop(AppMessage::CancelSelectFile)
                .into()
        } else {
            main
        }
    }
}
