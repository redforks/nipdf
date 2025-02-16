use clap::Parser;
use iced::{
    Element, Length, Task,
    alignment::Horizontal,
    application,
    widget::{Button, Row, Text, text_input},
};
use iced_aw::Card;
use log::error;
use mimalloc::MiMalloc;
use prescript::AnyWhatever;
use std::sync::Arc;
use view::{
    error::ErrorView,
    viewer::{Viewer, ViewerMessage},
    welcome::Welcome,
};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

mod app_state;
mod view;
pub(crate) use app::Message as AppMessage;

type Result<T, E = AnyWhatever> = std::result::Result<T, E>;

const APP_NAME: &str = "nipdf";

#[derive(Parser, Debug, Clone)]
struct Opts {
    #[arg(help = "PDF file name")]
    filename: Option<String>,

    #[arg(short, long, help = "Password")]
    password: Option<String>,
}

fn main() -> iced::Result {
    env_logger::init();

    let opts = Opts::parse();
    application("Nipdf Reader", app::update, app::view)
        .run_with(|| (app::State::new(opts), Task::none()))
}

mod app {
    use super::*;

    /// Messages for application view.
    #[derive(Debug, Clone)]
    pub(super) enum Message {
        Viewer(ViewerMessage),

        SelectFile,
        SelectedFileChange(String),
        CancelSelectFile,
        FileSelected,
    }

    #[derive(Default)]
    pub(super) struct State {
        current: View,
        selecting_file: bool,
        file_path_selecting: String,
        password: String,
    }

    impl State {
        pub fn new(opts: Opts) -> Self {
            let current = if let Some(filename) = opts.filename {
                let viewer = Viewer::new(filename, opts.password.unwrap_or_default());
                match viewer {
                    Ok(v) => View::Viewer(Box::new(v)),
                    Err(e) => View::Error(ErrorView::new(&e)),
                }
            } else {
                View::default()
            };

            Self {
                current,
                selecting_file: false,
                file_path_selecting: "".to_owned(),
                password: "".to_owned(),
            }
        }

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

        fn handle_result<T, E: std::fmt::Display>(&mut self, rv: Result<T, E>) -> Option<T> {
            match rv {
                Ok(v) => Some(v),
                Err(e) => {
                    self.current = View::Error(ErrorView::new(&e));
                    self.selecting_file = false;
                    None
                }
            }
        }

        fn open(&mut self) {
            let file_path = &self.file_path_selecting;
            let password = &self.password;
            if let Some(viewer) = self.handle_result(Viewer::new(file_path, password)) {
                self.current = View::Viewer(Box::new(viewer));
                app_state::save_last_file(&self.file_path_selecting);
            }
        }
    }

    pub(super) fn update(state: &mut State, message: Message) {
        match message {
            Message::Viewer(msg) => {
                if let Some(v) = state.mut_viewer() {
                    let rv = v.update(msg);
                    state.handle_result(rv);
                }
            }

            Message::SelectFile => {
                state.selecting_file = true;
                if let Some(viewer) = state.viewer() {
                    state.file_path_selecting = viewer.file_path().to_owned();
                }
            }
            Message::SelectedFileChange(path) => {
                state.file_path_selecting = path;
            }
            Message::CancelSelectFile => {
                state.selecting_file = false;
            }
            Message::FileSelected => {
                state.open();
                state.selecting_file = false;
            }
        }
    }

    pub(super) fn view(state: &State) -> Element<'_, Message> {
        let main = match &state.current {
            View::Viewer(v) => v.view(),
            View::Error(v) => v.view(),
            View::Welcome => Welcome::view(),
        };

        // if state.selecting_file {
        //     modal(main, Some(state.file_modal_view()))
        //         .on_esc(Message::CancelSelectFile)
        //         .backdrop(Message::CancelSelectFile)
        //         .into()
        // } else {
        main
        // }
    }
}

#[derive(Default)]
enum View {
    Error(ErrorView),
    Viewer(Box<Viewer>),
    #[default]
    Welcome,
}

// struct App {
//     current: View,
//     selecting_file: bool,
//     file_path_selecting: String,
//     password: String,
// }

// impl App {
//     fn viewer(&self) -> Option<&Viewer> {
//         match self.current {
//             View::Viewer(ref v) => Some(v),
//             _ => None,
//         }
//     }

//     fn mut_viewer(&mut self) -> Option<&mut Viewer> {
//         match self.current {
//             View::Viewer(ref mut v) => Some(v),
//             _ => None,
//         }
//     }

//     fn file_modal_view(&self) -> Element<'_, AppMessage> {
//         Card::new(
//             Text::new(APP_NAME),
//             text_input("pdf file path", &self.file_path_selecting)
//                 .on_input(AppMessage::SelectedFileChange)
//                 .on_submit(AppMessage::FileSelected),
//         )
//         .foot(
//             Row::new()
//                 .spacing(10)
//                 .padding(5)
//                 .width(Length::Fill)
//                 .push(
//                     Button::new(Text::new("Cancel").horizontal_alignment(Horizontal::Center))
//                         .width(Length::Fill)
//                         .on_press(AppMessage::CancelSelectFile),
//                 )
//                 .push(
//                     Button::new(Text::new("Ok").horizontal_alignment(Horizontal::Center))
//                         .width(Length::Fill)
//                         .on_press(AppMessage::FileSelected),
//                 ),
//         )
//         .max_width(300.0)
//         .on_close(AppMessage::CancelSelectFile)
//         .into()
//     }

//     fn handle_result<T, E: std::fmt::Display>(&mut self, rv: Result<T, E>) -> Option<T> {
//         match rv {
//             Ok(v) => Some(v),
//             Err(e) => {
//                 self.current = View::Error(ErrorView::new(&e));
//                 self.selecting_file = false;
//                 None
//             }
//         }
//     }

//     fn open_last_file(&mut self) {
//         if let Some(p) = app_state::load_last_file() {
//             match Viewer::new(p, &self.password) {
//                 Ok(v) => {
//                     self.current = View::Viewer(Box::new(v));
//                 }
//                 Err(e) => {
//                     error!("open last file failed: {}", e);
//                 }
//             }
//         }
//     }

//     fn open(&mut self) {
//         let file_path = &self.file_path_selecting;
//         if let Some(viewer) = self.handle_result(Viewer::new(file_path, &self.password)) {
//             self.current = View::Viewer(Box::new(viewer));
//             app_state::save_last_file(&self.file_path_selecting);
//         }
//     }
// }

// impl Application for App {
//     type Executor = executor::Default;
//     type Flags = Opts;
//     type Message = AppMessage;
//     type Theme = Theme;

//     fn new(flags: Self::Flags) -> (Self, Command<Self::Message>) {
//         let mut r = Self {
//             current: View::Welcome,
//             selecting_file: false,
//             file_path_selecting: "".to_owned(),
//             password: "".to_owned(),
//         };
//         if let Some(path) = flags.filename {
//             r.file_path_selecting = path;
//             r.password = flags.password.unwrap_or_default();
//             r.open();
//         } else {
//             r.open_last_file();
//         }
//         (
//             r,
//             // load icon font for iced_aw, without this modal close button icon will not show.
//             font::load(iced_aw::core::icons::BOOTSTRAP_FONT_BYTES).map(|_|
// AppMessage::Initialized),         )
//     }

//     fn title(&self) -> String {
//         self.viewer().map_or(APP_NAME.to_owned(), |v| {
//             format!("{APP_NAME} - {}", v.file_path())
//         })
//     }

//     fn update(&mut self, message: AppMessage) -> Command<Self::Message> {
//         match message {
//             AppMessage::Initialized => {}
//             AppMessage::Viewer(msg) => {
//                 if let Some(v) = self.mut_viewer() {
//                     let rv = v.update(msg);
//                     self.handle_result(rv);
//                 }
//             }

//             AppMessage::SelectFile => {
//                 self.selecting_file = true;
//                 if let Some(viewer) = self.viewer() {
//                     self.file_path_selecting = viewer.file_path().to_owned();
//                 }
//             }
//             AppMessage::SelectedFileChange(path) => {
//                 self.file_path_selecting = path;
//             }
//             AppMessage::CancelSelectFile => {
//                 self.selecting_file = false;
//             }
//             AppMessage::FileSelected => {
//                 self.open();
//                 self.selecting_file = false;
//             }
//         }

//         Command::none()
//     }

//     fn view(&self) -> Element<'_, AppMessage> {
//         let main = match &self.current {
//             View::Viewer(v) => v.view(),
//             View::Error(v) => v.view(),
//             View::Welcome => Welcome::view(),
//         };

//         if self.selecting_file {
//             modal(main, Some(self.file_modal_view()))
//                 .on_esc(AppMessage::CancelSelectFile)
//                 .backdrop(AppMessage::CancelSelectFile)
//                 .into()
//         } else {
//             main
//         }
//     }
// }
