//! View on application has error

use crate::AppMessage;
use iced::{
    Element,
    widget::{Text, button, row},
};

/// View on application has error
pub struct ErrorView(String);

impl ErrorView {
    pub fn new(err: impl ToString) -> Self {
        Self(err.to_string())
    }

    pub(crate) fn view(&self) -> Element<AppMessage> {
        row![
            Text::new(self.0.to_string()),
            button("Open a new file...").on_press(AppMessage::SelectFile),
        ]
        .into()
    }
}
