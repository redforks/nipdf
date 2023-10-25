use std::sync::Arc;
#[cfg(feature = "debug")]
use std::time::{Duration, Instant};

use crate::{AppMessage, ShardedData};
use anyhow::Result;
use iced::{
    alignment,
    widget::{
        button, column, horizontal_rule, horizontal_space,
        image::{Handle, Image},
        row, scrollable,
        scrollable::{Direction, Properties},
        text, text_input,
    },
    Length,
};
use iced::{Color, Element};
use iced_aw::{menu_bar, native::helpers::menu_tree, native::menu::MenuTree};
use nipdf::file::{File as PdfFile, ObjectResolver, RenderOptionBuilder, XRefTable};

#[derive(Clone, Debug, Copy)]
struct PageNavigator {
    current_page: u32,
    total_pages: u32,
}

impl PageNavigator {
    pub fn next(&mut self) {
        if (self.current_page + 1) < self.total_pages {
            self.current_page += 1;
        }
    }

    pub fn prev(&mut self) {
        if self.current_page > 0 {
            self.current_page -= 1;
        }
    }

    pub fn can_next(&self) -> bool {
        (self.current_page + 1) < self.total_pages
    }

    pub fn can_prev(&self) -> bool {
        self.current_page > 0
    }
}

/// Current displayed Pdf rendered page.
struct Page {
    width: u32,
    height: u32,
    data: ShardedData,
}

/// Messages for pdf file viewer view.
#[derive(Debug, Clone)]
pub enum ViewerMessage {
    NextPage,
    PrevPage,
    ZoomIn,
    ZoomOut,

    CurPageChange(String),
    CurPageChanged,

    #[cfg(feature = "debug")]
    ShowPageId,
    #[cfg(feature = "debug")]
    Todo,
}

/// Pdf file viewer
pub struct Viewer {
    file_path: String,
    page: Page,
    navi: PageNavigator,
    zoom: f32,
    cur_page_editing: String,
    #[cfg(feature = "debug")]
    render_time: Duration,
    file_data: Vec<u8>,
    xref: XRefTable,
    file: PdfFile,
}

impl Viewer {
    pub fn new(file_path: impl Into<String>) -> Result<Self> {
        let file_path = file_path.into();
        let file_data = std::fs::read(&file_path)?;
        let (file, xref) = PdfFile::parse(&file_data[..])?;
        let mut r = Self {
            file_path,
            page: Page {
                width: 0,
                height: 0,
                data: ShardedData(Arc::new(vec![])),
            },
            navi: PageNavigator {
                current_page: 0,
                total_pages: 0,
            },
            zoom: 1.75,
            cur_page_editing: "".to_owned(),
            #[cfg(feature = "debug")]
            render_time: Duration::default(),
            xref,
            file,
            file_data,
        };
        r.load_page(0)?;
        Ok(r)
    }

    pub fn file_path(&self) -> &str {
        &self.file_path
    }

    fn update_cur_page_editing_from_navigation(&mut self) {
        self.cur_page_editing = format!("{}", self.navi.current_page + 1);
    }

    fn load_page(&mut self, no: u32) -> Result<()> {
        #[cfg(feature = "debug")]
        let now = Instant::now();
        let resolver = ObjectResolver::new(&self.file_data, &self.xref);
        let catalog = self.file.catalog(&resolver)?;
        let pages = catalog.pages()?;
        let page = &pages[no as usize];
        let option = RenderOptionBuilder::new().zoom(self.zoom);
        let page = page.render(option)?;
        self.page = Page {
            width: page.width(),
            height: page.height(),
            data: ShardedData(Arc::new(page.take())),
        };
        self.navi = PageNavigator {
            current_page: no,
            total_pages: pages.len() as u32,
        };
        self.update_cur_page_editing_from_navigation();
        #[cfg(feature = "debug")]
        {
            self.render_time = now.elapsed();
        }
        Ok(())
    }

    #[cfg(feature = "debug")]
    fn page_object_number(&self) -> Result<u32> {
        let resolver = ObjectResolver::new(&self.file_data, &self.xref);
        let catalog = self.file.catalog(&resolver)?;
        let pages = catalog.pages()?;
        let page = &pages[self.navi.current_page as usize];
        Ok(page.id())
    }

    pub fn update(&mut self, message: ViewerMessage) -> Result<()> {
        match message {
            ViewerMessage::NextPage => {
                self.navi.next();
                self.load_page(self.navi.current_page)
            }
            ViewerMessage::PrevPage => {
                self.navi.prev();
                self.load_page(self.navi.current_page)
            }
            ViewerMessage::ZoomIn => {
                self.zoom *= 1.25;
                self.load_page(self.navi.current_page)
            }
            ViewerMessage::ZoomOut => {
                self.zoom /= 1.25;
                self.load_page(self.navi.current_page)
            }
            ViewerMessage::CurPageChange(s) => {
                self.cur_page_editing = s;
                Ok(())
            }
            ViewerMessage::CurPageChanged => {
                if let Ok(page) = self.cur_page_editing.parse::<u32>() {
                    if page > 0 && page <= self.navi.total_pages {
                        self.navi.current_page = page - 1;
                        self.load_page(self.navi.current_page)
                    } else {
                        self.update_cur_page_editing_from_navigation();
                        Ok(())
                    }
                } else {
                    self.update_cur_page_editing_from_navigation();
                    Ok(())
                }
            }
            #[cfg(feature = "debug")]
            ViewerMessage::ShowPageId => {
                use notify_rust::Notification;
                let id = self.page_object_number()?;
                Notification::new()
                    .appname(crate::APP_NAME)
                    .summary(&id.to_string())
                    .show()?;
                Ok(())
            }
            #[cfg(feature = "debug")]
            ViewerMessage::Todo => {
                use notify_rust::Notification;
                Notification::new()
                    .appname(crate::APP_NAME)
                    .summary("TODO")
                    .show()?;
                Ok(())
            }
        }
    }

    pub(crate) fn view(&self) -> Element<AppMessage> {
        column![
            row(vec![
                // can not use row! macro, it has compile problems because of #[cfg] attribute on some of items
                button("Open...").on_press(AppMessage::SelectFile).into(),
                horizontal_space(16).into(),
                text_input("Page", &self.cur_page_editing)
                    .width(60)
                    .on_input(|s| AppMessage::Viewer(ViewerMessage::CurPageChange(s)))
                    .on_submit(AppMessage::Viewer(ViewerMessage::CurPageChanged))
                    .into(),
                text(format!("/{}", self.navi.total_pages)).into(),
                horizontal_space(16).into(),
                button("Prev")
                    .on_press_maybe(
                        self.navi
                            .can_prev()
                            .then_some(AppMessage::Viewer(ViewerMessage::PrevPage))
                    )
                    .into(),
                button("Next")
                    .on_press_maybe(
                        self.navi
                            .can_next()
                            .then_some(AppMessage::Viewer(ViewerMessage::NextPage))
                    )
                    .into(),
                horizontal_space(16).into(),
                button("Zoom In")
                    .on_press(AppMessage::Viewer(ViewerMessage::ZoomIn))
                    .into(),
                button("Zoom Out")
                    .on_press(AppMessage::Viewer(ViewerMessage::ZoomOut))
                    .into(),
                horizontal_space(Length::Fill).into(),
                #[cfg(feature = "debug")]
                text(format!("{} ms", self.render_time.as_millis())).into(),
                #[cfg(feature = "debug")]
                horizontal_space(8).into(),
                #[cfg(feature = "debug")]
                menu_bar!(menu_tree(
                    text("Debug"),
                    vec![
                        new_menu_item("Page Object id", ViewerMessage::ShowPageId),
                        MenuTree::new(horizontal_rule(8)),
                        new_menu_item("Page Object", ViewerMessage::Todo),
                        new_menu_item("Page Content", ViewerMessage::Todo),
                        new_menu_item("Page Stream", ViewerMessage::Todo),
                        MenuTree::new(horizontal_rule(8)),
                        new_menu_item("Dump Page", ViewerMessage::Todo),
                    ]
                ))
                .into()
            ])
            .align_items(iced::Alignment::Center),
            scrollable(
                Image::new(Handle::from_pixels(
                    self.page.width,
                    self.page.height,
                    self.page.data.clone(),
                ))
                .content_fit(iced::ContentFit::None)
            )
            .direction(Direction::Both {
                vertical: Properties::default(),
                horizontal: Properties::default(),
            })
        ]
        .into()
    }
}

fn new_menu_item(label: &str, message: ViewerMessage) -> MenuTree<AppMessage, iced::Renderer> {
    MenuTree::new(
        button(
            text(label)
                .width(Length::Fill)
                .height(Length::Fill)
                .vertical_alignment(alignment::Vertical::Center),
        )
        .padding([4, 8])
        .style(iced::theme::Button::Custom(Box::new(ButtonStyle {})))
        .on_press(AppMessage::Viewer(message)),
    )
}

struct ButtonStyle;
impl button::StyleSheet for ButtonStyle {
    type Style = iced::Theme;

    fn active(&self, style: &Self::Style) -> button::Appearance {
        button::Appearance {
            text_color: style.extended_palette().background.base.text,
            border_radius: [4.0; 4].into(),
            background: Some(Color::TRANSPARENT.into()),
            ..Default::default()
        }
    }

    fn hovered(&self, style: &Self::Style) -> button::Appearance {
        let plt = style.extended_palette();

        button::Appearance {
            background: Some(plt.primary.weak.color.into()),
            text_color: plt.primary.weak.text,
            ..self.active(style)
        }
    }
}
