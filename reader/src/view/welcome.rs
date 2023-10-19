//! Welcome view when no file is opened.

use iced::widget::{button, column, container, text};
use iced::{Element, Length};

use crate::AppMessage;

/// Welcome view when no file is opened.
pub struct Welcome;

impl Welcome {
    pub(crate) fn view(&self) -> Element<AppMessage> {
        let content = column![
            text("Welcome to nipdf!"),
            button("Open a pdf file").on_press(AppMessage::SelectFile),
        ]
        .align_items(iced::Alignment::Center);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x()
            .center_y()
            .into()
    }
}
