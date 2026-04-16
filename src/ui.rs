use ratatui::{
    layout::{Layout, HorizontalAlignment, Constraint, Rect, Flex},
    widgets::{Block, Borders, Paragraph, Wrap, Scrollbar, ScrollbarState, ScrollbarOrientation, Widget, Clear},
    style::{Style, Stylize, Color},
    text::{Span, Line, Text},
    buffer::Buffer,
    Frame
};
use ratatui_textarea::TextArea;

use crate::Shared;
use crate::env::CHATTY_VERSION;

#[derive(Default)]
pub struct Popup<'a> {
    title: Line<'a>,
    title_bottom: Line<'a>,
    content: Text<'a>,
    border_style: Style,
    title_style: Style,
    style: Style,
    title_alignment: HorizontalAlignment,
}

impl<'a> Popup<'a> {
    fn title<T: Into<Line<'a>>>(mut self, title: T) -> Self {
        let title = title.into();
        self.title = title;
        self
    }

    fn title_bottom<T: Into<Line<'a>>>(mut self, title: T) -> Self {
        let title = title.into();
        self.title_bottom = title;
        self
    }

    fn title_alignment(mut self, alignment: HorizontalAlignment) -> Self {
        self.title_alignment = alignment;
        self
    }

    fn content<T: Into<Text<'a>>>(mut self, content: T) -> Self {
        let content = content.into();
        self.content = content;
        self
    }

    fn border_style<T: Into<Style>>(mut self, style: T) -> Self {
        let style = style.into();
        self.border_style = style;
        self
    }

    fn title_style<T: Into<Style>>(mut self, style: T) -> Self {
        let style = style.into();
        self.title_style = style;
        self
    }

    fn style<T: Into<Style>>(mut self, style: T) -> Self {
        let style = style.into();
        self.style = style;
        self
    }

    fn render_centered(self, area: Rect, buf: &mut Buffer) {
        let [vertical] = Layout::vertical([Constraint::Max(10)])
            .flex(Flex::Center)
            .areas(area);
        let [horizontal] = Layout::horizontal([Constraint::Max(40)])
            .flex(Flex::Center)
            .areas(vertical);

        self.render(horizontal, buf);
    }
}

impl Widget for Popup<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);
        let block = Block::bordered()
            .title(self.title)
            .title_bottom(self.title_bottom)
            .title_style(self.title_style)
            .title_alignment(self.title_alignment)
            .border_style(self.border_style);

        Paragraph::new(self.content)
            .wrap(Wrap { trim: true })
            .style(self.style)
            .block(block)
            .render(area, buf);
    }
}

pub enum ActivePopup {
    Info(String),
    Error(String)
}

impl ActivePopup {
    pub fn draw(&self, frame: &mut Frame) {
        let area = frame.area();
        match self {
            ActivePopup::Info(message) =>
                Popup::default()
                    .content(message.as_str())
                    .style(Style::default())
                    .title_alignment(HorizontalAlignment::Center)
                    .title(" Info ")
                    .title_bottom(vec![" <ESC> ".light_blue(), "dismiss ".dark_gray()])
                    .title_style(Style::default().light_blue())
                    .border_style(Style::default().light_blue())
                    .render_centered(area, frame.buffer_mut()),
            ActivePopup::Error(message) =>
                Popup::default()
                    .content(message.as_str())
                    .style(Style::default())
                    .title_alignment(HorizontalAlignment::Center)
                    .title(" Error ")
                    .title_bottom(vec![" <ESC> ".light_red(), "dismiss ".dark_gray()])
                    .title_style(Style::default().light_red())
                    .border_style(Style::default().light_red())
                    .render_centered(area, frame.buffer_mut())
        }
    }
}

#[derive(Default, PartialEq)]
pub enum ConnectFormField {
    #[default]
    Address,
    Name
}

#[derive(Default)]
pub struct ConnectForm {
    pub focused: ConnectFormField
}

impl ConnectForm {
    pub fn next_field(&mut self) {
        self.focused = match self.focused {
            ConnectFormField::Address => ConnectFormField::Name,
            ConnectFormField::Name => ConnectFormField::Address
        }
    }
}

pub struct Ui {
    pub address_input_box: TextArea<'static>,
    pub name_input_box: TextArea<'static>,
    pub chat_input_box: TextArea<'static>,
    vertical_scroll_state: ScrollbarState,
    vertical_scroll: usize,
    max_scroll: usize,
    auto_scroll: bool,
    pub connect_form: ConnectForm
}

impl Ui {
    pub fn draw_connect_form(&mut self, frame: &mut Frame) {
        let [vertical] = Layout::vertical([Constraint::Max(10)])
            .flex(Flex::Center)
            .areas(frame.area());

        let [horizontal] = Layout::horizontal([Constraint::Max(40)])
            .flex(Flex::Center)
            .areas(vertical);

        let container = Block::bordered()
            .title(format!(" ChaTTY ({CHATTY_VERSION}) - Connect "))
            .title_alignment(HorizontalAlignment::Center)
            .title_bottom(vec![
                " <TAB> ".yellow(),
                "next ".dark_gray(),
                "|".reset(),
                " <ESC> ".yellow(),
                "exit ".dark_gray()
            ])
            .yellow();

        let container_inner = container.inner(horizontal);

        frame.render_widget(container, horizontal);

        let chunks = Layout::vertical([Constraint::Length(3), Constraint::Length(3)])
            .flex(Flex::Center)
            .margin(1)
            .split(container_inner);

        self.draw_connect_form_address(frame, chunks[0]);
        self.draw_connect_form_name(frame, chunks[1]);
    }

    fn draw_connect_form_address(&mut self, frame: &mut Frame, field_address_area: Rect) {
        let color = if self.connect_form.focused == ConnectFormField::Address { Color::Yellow } else { Color::DarkGray };

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Address ")
            .fg(color);

        self.address_input_box.set_block(block);
        self.address_input_box.set_cursor_line_style(Style::default().fg(Color::Reset));
        self.address_input_box.set_placeholder_text("Enter the server address...");

        if self.connect_form.focused == ConnectFormField::Address {
            self.address_input_box.set_cursor_style(Style::default().reversed());
        } else {
            self.address_input_box.set_cursor_style(Style::default());
        }

        frame.render_widget(&self.address_input_box, field_address_area);
    }

    fn draw_connect_form_name(&mut self, frame: &mut Frame, field_address_name: Rect) {
        let color = if self.connect_form.focused == ConnectFormField::Name { Color::Yellow } else { Color::DarkGray };

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Name ")
            .fg(color);

        self.name_input_box.set_block(block);
        self.name_input_box.set_cursor_line_style(Style::default().fg(Color::Reset));
        self.name_input_box.set_placeholder_text("Enter your name...");

        if self.connect_form.focused == ConnectFormField::Name {
            self.name_input_box.set_cursor_style(Style::default().reversed());
        } else {
            self.name_input_box.set_cursor_style(Style::default());
        }

        frame.render_widget(&self.name_input_box, field_address_name);
    }

    pub fn draw_chat(&mut self, frame: &mut Frame, shared: &Shared) {
        let chunks = Layout::vertical([Constraint::Min(1), Constraint::Length(3)])
            .margin(1)
            .split(frame.area());

        self.draw_chat_window(frame, chunks[0], shared);
        self.draw_chat_input_box(frame, chunks[1], shared);
    }

    fn draw_chat_window(&mut self, frame: &mut Frame, chat_window_area: Rect, shared: &Shared) {
        let messages = shared.messages.lock().unwrap();

        let block = Block::bordered()
            .title(" ChaTTY ".yellow())
            .title_alignment(HorizontalAlignment::Center);

        let chat = Paragraph::new(messages.clone())
            .wrap(Wrap { trim: true })
            .block(block)
            .scroll((self.vertical_scroll as u16, 0));

        let line_count = chat.line_count(chat_window_area.width - 2);
        let visible_lines = (chat_window_area.height) as usize;
        self.max_scroll = line_count.saturating_sub(visible_lines);

        if self.vertical_scroll > self.max_scroll || self.auto_scroll {
            self.vertical_scroll = self.max_scroll;
        }
        self.vertical_scroll_state = self.vertical_scroll_state
            .content_length(self.max_scroll)
            .position(self.vertical_scroll);

        frame.render_widget(chat, chat_window_area);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            chat_window_area,
            &mut self.vertical_scroll_state,
        );
    }

    fn draw_chat_input_box(&mut self, frame: &mut Frame, input_box_area: Rect, shared: &Shared) {
        let name = shared.name.lock().unwrap().to_owned();
        let color: Color = shared.color.lock().unwrap().to_owned().into();

        let block = Block::bordered()
            .title(vec![" You (".into(), Span::styled(name, Style::default().fg(color)), "): ".into()])
            .yellow();

        self.chat_input_box.set_block(block);
        self.chat_input_box.set_cursor_line_style(Style::default().fg(Color::Reset));
        self.chat_input_box.set_placeholder_text("Your message...");

        frame.render_widget(&self.chat_input_box, input_box_area);
    }

    pub fn vertical_scroll_up(&mut self) {
        self.vertical_scroll = self.vertical_scroll.saturating_sub(1);
        self.vertical_scroll_state = self.vertical_scroll_state.position(self.vertical_scroll);
        self.auto_scroll = false;
    }

    pub fn vertical_scroll_down(&mut self) {
        self.vertical_scroll = self.vertical_scroll.saturating_add(1);
        self.vertical_scroll_state = self.vertical_scroll_state.position(self.vertical_scroll);
        if self.vertical_scroll >= self.max_scroll {
            self.auto_scroll = true;
        }
    }
}

impl Default for Ui {
    fn default() -> Self {
        Self {
            connect_form: ConnectForm::default(),
            address_input_box: TextArea::default(),
            name_input_box: TextArea::default(),
            chat_input_box: TextArea::default(),
            vertical_scroll_state: ScrollbarState::default(),
            vertical_scroll: 0,
            max_scroll: 0,
            auto_scroll: true
        }
    }
}
