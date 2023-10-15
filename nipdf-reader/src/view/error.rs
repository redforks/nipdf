//! View on application has error

use iced::{
    widget::{button, row, Text},
    Element,
};

use crate::AppMessage;

/// View on application has error
pub struct ErrorView(String);

impl ErrorView {
    pub fn new(err: impl ToString) -> Self {
        Self(err.to_string())
    }

    pub(crate) fn view(&self) -> Element<AppMessage> {
        row![
            Text::new(format!("{}", &self.0)),
            button("Open a new file...").on_press(AppMessage::SelectFile),
        ]
        .into()
    }
}
