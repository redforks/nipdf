use clap::Parser;
use iced::{Element, Task, application};
use mimalloc::MiMalloc;
use prescript::AnyWhatever;
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

impl From<Opts> for Option<(String, String)> {
    fn from(value: Opts) -> Self {
        if let Some(filename) = value.filename {
            Some((filename, value.password.unwrap_or_default()))
        } else {
            None
        }
    }
}

fn main() -> iced::Result {
    env_logger::init();

    let opts = Opts::parse();
    application("Nipdf Reader", app::update, app::view)
        .run_with(|| (app::State::new(opts), Task::none()))
}

mod app {
    use super::*;
    use crate::app_state::load_last_file;
    use rfd::FileDialog;
    use snafu::OptionExt as _;

    /// Messages for application view.
    #[derive(Debug, Clone)]
    pub(super) enum Message {
        Viewer(ViewerMessage),

        SelectFile,
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
            let args: Option<(String, String)> =
                <Option<(String, String)>>::from(opts).or_else(load_last_file);
            let current = if let Some((filename, password)) = args {
                let viewer = Viewer::new(filename, password);
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
                let filename = FileDialog::new().add_filter("PDF", &["pdf"]).pick_file();
                if let Some(filename) = filename {
                    if let Some(filename) = state.handle_result(
                        filename
                            .to_str()
                            .map(ToString::to_string)
                            .whatever_context::<_, AnyWhatever>("get file path"),
                    ) {
                        state.file_path_selecting = filename;
                        state.password = String::new();
                        state.open();
                    }
                }
            }
        }
    }

    pub(super) fn view(state: &State) -> Element<'_, Message> {
        let main = match &state.current {
            View::Viewer(v) => v.view(),
            View::Error(v) => v.view(),
            View::Welcome => Welcome::view(),
        };

        main
    }
}

#[derive(Default)]
enum View {
    Error(ErrorView),
    Viewer(Box<Viewer>),
    #[default]
    Welcome,
}
