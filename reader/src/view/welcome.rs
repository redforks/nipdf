//! Welcome view when no file is opened.

use crate::AppMessage;
use iced::{
    Element, Length,
    widget::{button, column, container, text},
};

/// Welcome view when no file is opened.
pub struct Welcome;

impl Welcome {
    pub(crate) fn view() -> Element<'static, AppMessage> {
        let content = column![
            text("Welcome to nipdf!"),
            button("Open a pdf file").on_press(AppMessage::SelectFile),
        ]
        .align_x(iced::Alignment::Center);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .center(iced::Length::Fill)
            .into()
    }
}
