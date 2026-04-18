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

#[derive(Default)]
struct ScrollState {
    bar: ScrollbarState,
    scroll: usize,
    max: usize,
    auto: bool
}

impl ScrollState {
    fn update_from_paragraph(&mut self, paragraph: &Paragraph<'_>, area: Rect) {
        let line_count = paragraph.line_count(area.width - 2);
        let visible_lines = area.height as usize;
        self.max = line_count.saturating_sub(visible_lines);

        if self.scroll > self.max || self.auto {
            self.scroll = self.max;
        }
        self.bar = self.bar
            .content_length(self.max)
            .position(self.scroll);
    }

    fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
        self.bar = self.bar.position(self.scroll);
        self.auto = false;
    }

    fn scroll_down(&mut self) {
        self.scroll = self.scroll.saturating_add(1);
        self.bar = self.bar.position(self.scroll);
        if self.scroll >= self.max {
            self.auto = true;
        }
    }
}

pub struct Ui {
    pub address_input_box: TextArea<'static>,
    pub name_input_box: TextArea<'static>,
    pub chat_input_box: TextArea<'static>,
    chat_scroll: ScrollState,
    client_list_scroll: ScrollState,
    pub connect_form: ConnectForm,
    pub chat_window_area: Rect,
    pub client_list_area: Rect
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

        let [address_field, name_field] = Layout::vertical([Constraint::Length(3), Constraint::Length(3)])
            .flex(Flex::Center)
            .margin(1)
            .areas(container_inner);

        self.draw_connect_form_address(frame, address_field);
        self.draw_connect_form_name(frame, name_field);
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
        /*
        *  |---------------------------------| |------------|
        *  | The chat...                     | |    List    |
        *  |                                 | |            |
        *  |                                 | |            |
        *  |                                 | |            |
        *  |                                 | |            |
        *  |                                 | |            |
        *  |                                 | |            |
        *  |---------------------------------| |------------|
        *  |------------------------------------------------|
        *  |Input box                                       |
        *  |________________________________________________|
        */

        // Split the screen in top and bottom (`top_area` and `input_box_area`) and then later split
        // the `top_area` to `chat_window_area` and `client_list_area`

        let [top_area, input_box_area] = Layout::vertical([
            Constraint::Min(3),
            Constraint::Length(3)
        ])
        .margin(1)
        .areas(frame.area());

        let [chat_window_area, client_list_area] = Layout::horizontal([
            Constraint::Min(3),
            Constraint::Length(15)
        ])
        .spacing(1)
        .areas(top_area);

        self.chat_window_area = chat_window_area;
        self.client_list_area = client_list_area;

        self.draw_chat_window(frame, shared);
        self.draw_client_list(frame, shared);
        self.draw_chat_input_box(frame, input_box_area, shared);
    }

    fn draw_client_list(&mut self, frame: &mut Frame, shared: &Shared) {
        let block = Block::bordered()
            .title(" Clients ".yellow())
            .title_alignment(HorizontalAlignment::Center);

        let lock = shared.clients.lock().unwrap();
        let mut clients = Vec::with_capacity(lock.len());

        for (name, color) in lock.iter() {
            let line = Line::styled(
                format!("• {name}"),
                Style::default().fg((*color).into())
            );
            clients.push(line);
        }

        let list = Paragraph::new(clients)
            .wrap(Wrap { trim: true })
            .block(block)
            .scroll((self.client_list_scroll.scroll as u16, 0));

        self.client_list_scroll.update_from_paragraph(&list, self.client_list_area);

        frame.render_widget(list, self.client_list_area);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            self.client_list_area,
            &mut self.client_list_scroll.bar);
    }

    fn draw_chat_window(&mut self, frame: &mut Frame, shared: &Shared) {
        let messages = shared.messages.lock().unwrap();

        let block = Block::bordered()
            .title(" ChaTTY ".yellow())
            .title_alignment(HorizontalAlignment::Center);

        let chat = Paragraph::new(messages.clone())
            .wrap(Wrap { trim: true })
            .block(block)
            .scroll((self.chat_scroll.scroll as u16, 0));

        self.chat_scroll.update_from_paragraph(&chat, self.chat_window_area);

        frame.render_widget(chat, self.chat_window_area);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            self.chat_window_area,
            &mut self.chat_scroll.bar,
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

    pub fn chat_scroll_up(&mut self) {
        self.chat_scroll.scroll_up();
    }

    pub fn chat_scroll_down(&mut self) {
        self.chat_scroll.scroll_down();
    }

    pub fn client_list_scroll_up(&mut self) {
        self.client_list_scroll.scroll_up();
    }

    pub fn client_list_scroll_down(&mut self) {
        self.client_list_scroll.scroll_down();
    }
}

impl Default for Ui {
    fn default() -> Self {
        Self {
            connect_form: ConnectForm::default(),
            address_input_box: TextArea::default(),
            name_input_box: TextArea::default(),
            chat_input_box: TextArea::default(),
            chat_scroll: ScrollState {
                auto: true,
                ..Default::default()
            },
            client_list_scroll: ScrollState::default(),
            chat_window_area: Rect::default(),
            client_list_area: Rect::default()
        }
    }
}
