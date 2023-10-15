use std::sync::Arc;

use anyhow::Result;
use iced::{
    alignment::Horizontal,
    widget::{text_input, Button, Row, Text},
    Length,
};
use iced::{Element, Sandbox, Settings};
use iced_aw::{modal, Card};

mod view;
use view::{
    error::ErrorView,
    viewer::{Viewer, ViewerMessage},
    welcome::Welcome,
};

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

enum View {
    Error(ErrorView),
    Viewer(Viewer),
    Welcome(Welcome),
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

    fn open(&mut self, file_path: impl Into<String>) {
        if let Some(viewer) = self.handle_result(Viewer::new(file_path)) {
            self.current = View::Viewer(viewer);
        }
    }
}

impl Sandbox for App {
    type Message = AppMessage;

    fn new() -> Self {
        Self {
            current: View::Welcome(Welcome),
            selecting_file: false,
            file_path_selecting: "".to_owned(),
        }
    }

    fn title(&self) -> String {
        self.viewer()
            .map_or("nipdf".to_owned(), |v| format!("nipdf - {}", v.file_path()))
    }

    fn update(&mut self, message: AppMessage) {
        match message {
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
            AppMessage::FileSelected(path) => {
                self.open(path);
                self.selecting_file = false;
            }
        }
    }

    fn view(&self) -> Element<AppMessage> {
        let main = match &self.current {
            View::Viewer(v) => v.view(),
            View::Error(v) => v.view(),
            View::Welcome(v) => v.view(),
        };

        if self.selecting_file {
            modal(main, Some(self.file_modal_view())).into()
        } else {
            main
        }
    }
}
