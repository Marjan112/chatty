use ratatui::{
    layout::{Layout, HorizontalAlignment, Constraint, Rect, Flex, Margin},
    widgets::{Block, Borders, Paragraph, Wrap, Scrollbar, ScrollbarState, Widget, Clear, List, ListState},
    style::{Style, Stylize, Color},
    text::{Span, Line, Text},
    buffer::Buffer,
    Frame
};
use ratatui_textarea::{DataCursor, TextArea};

use crate::Shared;

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

struct ScrollState {
    bar: ScrollbarState,
    scroll: usize,
    max: usize,
    auto: bool
}

impl Default for ScrollState {
    fn default() -> Self {
        Self::new(true)
    }
}

impl ScrollState {
    fn new(auto: bool) -> Self {
        Self {
            bar: ScrollbarState::default(),
            scroll: 0,
            max: 0,
            auto
        }
    }

    fn update_from_paragraph(&mut self, paragraph: &Paragraph<'_>, area: Rect) {
        let line_count = paragraph.line_count(area.width.saturating_sub(2));
        let visible_lines = area.height as usize;
        self.max = line_count.saturating_sub(visible_lines);

        if self.scroll > self.max || self.auto {
            self.scroll = self.max;
        }
        self.bar = self.bar
            .content_length(self.max)
            .position(self.scroll);
    }

    fn scroll_up_by(&mut self, how_much: usize) {
        self.scroll = self.scroll.saturating_sub(how_much);
        self.bar = self.bar.position(self.scroll);
        self.auto = false;
    }

    fn scroll_down_by(&mut self, how_much: usize) {
        self.scroll = self.scroll.saturating_add(how_much);
        self.bar = self.bar.position(self.scroll);
        if self.scroll >= self.max {
            self.scroll = self.max;
            self.auto = true;
        }
    }

    fn scroll_up(&mut self) {
        self.scroll_up_by(3);
    }

    fn scroll_down(&mut self) {
        self.scroll_down_by(3);
    }

    fn page_up(&mut self, height: u16) {
        self.scroll_up_by(height as usize);
    }

    fn page_down(&mut self, height: u16) {
        self.scroll_down_by(height as usize);
    }
}

pub struct CompletionState {
    matches: Vec<&'static str>,
    list_state: ListState,
    scrollbar_state: ScrollbarState
}

impl CompletionState {
    pub fn new(matches: Vec<&'static str>) -> Self {
        let mut list_state = ListState::default();
        if !matches.is_empty() {
            list_state.select(Some(0));
        }

        Self {
            matches,
            list_state,
            scrollbar_state: ScrollbarState::default()
        }
    }

    pub fn next(&mut self) -> Option<&str> {
        if self.matches.is_empty() {
            return None;
        }

        let index = self.list_state
            .selected()
            .unwrap_or_default();

        let next_index = (index + 1) % self.matches.len();

        self.list_state.select(Some(next_index));
        Some(self.matches[next_index])
    }

    pub fn selected(&self) -> Option<&str> {
        self.list_state
            .selected()
            .map(|i| self.matches[i])
    }
}

#[derive(Default)]
pub struct Prompt {
    pub textarea: TextArea<'static>,
    history: Vec<String>,
    history_index: Option<usize>,
    saved_input: String,
    pub completion_state: Option<CompletionState>
}

impl Prompt {
    pub fn set_text<T: AsRef<str>>(&mut self, text: T) {
        self.textarea.clear();
        self.textarea.insert_str(text);
    }

    pub fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }

        match self.history_index {
            None => {
                self.saved_input = self.textarea.lines().join("");
                self.history_index = Some(self.history.len() - 1);
            }
            Some(0) => return,
            Some(index) => {
                self.history_index = Some(index - 1);
            }
        } 

        self.load_history();
    }

    pub fn history_next(&mut self) {
        match self.history_index {
            None => (),
            Some(index) if index < self.history.len() - 1 => {
                self.history_index = Some(index + 1);
                self.load_history();
            }
            Some(_) => {
                self.history_index = None;

                self.textarea.clear();
                self.textarea.insert_str(&self.saved_input);

                self.saved_input.clear();
            }
        }
    }

    pub fn add_to_history(&mut self, input: String) {
        if self.history.last() != Some(&input) {
            self.history.push(input);
        }

        self.history_index = None;
        self.saved_input.clear();
    }

    fn load_history(&mut self) {
        if let Some(index) = self.history_index {
            self.textarea.clear();
            self.textarea.insert_str(&self.history[index]);
        }
    }
}

#[derive(Default)]
pub struct Ui {
    pub address_input_box: TextArea<'static>,
    pub name_input_box: TextArea<'static>,
    pub chat_prompt: Prompt,
    chat_scroll: ScrollState,
    client_list_scroll: ScrollState,
    pub connect_form: ConnectForm,
    pub chat_window_area: Rect,
    pub client_list_area: Rect
}

impl Ui {
    pub fn draw_connect_form(&mut self, frame: &mut Frame) {
        let [vertical] = Layout::vertical([Constraint::Length(10)])
            .flex(Flex::Center)
            .areas(frame.area());

        let [horizontal] = Layout::horizontal([Constraint::Max(40)])
            .flex(Flex::Center)
            .areas(vertical);

        let container = Block::bordered()
            .title(" ChaTTY - Connect ")
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

        // Split the screen in top and bottom (`top_area` and `chat_prompt_area`) and then later split
        // the `top_area` to `chat_window_area` and `client_list_area`

        let [top_area, chat_prompt_area] = Layout::vertical([
            Constraint::Min(3),
            Constraint::Length(3)
        ])
        .areas(frame.area());

        let [chat_window_area, client_list_area] = Layout::horizontal([
            Constraint::Min(50),
            Constraint::Length(25)
        ])
        .areas(top_area);

        self.chat_window_area = chat_window_area;
        self.client_list_area = client_list_area;

        self.draw_chat_window(frame, shared);
        self.draw_client_list(frame, shared);
        self.draw_chat_prompt(frame, chat_prompt_area, shared);
    }

    fn draw_client_list(&mut self, frame: &mut Frame, shared: &Shared) {
        let clients_lock = shared.clients.lock().unwrap();
        let clients: Vec<_> = clients_lock
            .iter()
            .map(|(name, color)| Line::styled(name, Style::default().fg((*color).into())))
            .collect();

        let block = Block::bordered()
            .title(format!(" Clients ({}) ", clients.len()).yellow())
            .title_alignment(HorizontalAlignment::Center);

        let list = Paragraph::new(clients)
            .block(block)
            .scroll((self.client_list_scroll.scroll as u16, 0));

        self.client_list_scroll.update_from_paragraph(&list, self.client_list_area);

        frame.render_widget(list, self.client_list_area);
        frame.render_stateful_widget(
            Scrollbar::default()
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(None),
            self.client_list_area.inner(Margin::new(0, 1)),
            &mut self.client_list_scroll.bar);
    }

    fn draw_chat_window(&mut self, frame: &mut Frame, shared: &Shared) {
        let messages = shared.messages.lock().unwrap();

        let block = Block::bordered()
            .title(" Chat ".yellow())
            .title_alignment(HorizontalAlignment::Center);

        let chat = Paragraph::new(messages.clone())
            .wrap(Wrap { trim: true })
            .block(block)
            .scroll((self.chat_scroll.scroll as u16, 0));

        self.chat_scroll.update_from_paragraph(&chat, self.chat_window_area);

        frame.render_widget(chat, self.chat_window_area);
        frame.render_stateful_widget(
            Scrollbar::default()
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(None),
            self.chat_window_area.inner(Margin::new(0, 1)),
            &mut self.chat_scroll.bar,
        );
    }

    fn draw_chat_prompt(&mut self, frame: &mut Frame, chat_prompt_area: Rect, shared: &Shared) {
        let name = shared.name.lock().unwrap().to_owned();
        let color: Color = shared.color.lock().unwrap().to_owned().into();

        let block = Block::bordered()
            .title(vec![" You (".into(), Span::styled(name, Style::default().fg(color)), "): ".into()])
            .yellow();

        self.chat_prompt.textarea.set_block(block);
        self.chat_prompt.textarea.set_cursor_line_style(Style::default().fg(Color::Reset));
        self.chat_prompt.textarea.set_placeholder_text("Your message...");

        frame.render_widget(&self.chat_prompt.textarea, chat_prompt_area);
        
        self.draw_completion_popup(frame, chat_prompt_area);
    }

    fn draw_completion_popup(&mut self, frame: &mut Frame, chat_prompt_area: Rect) {
        if let Some(completion) = &mut self.chat_prompt.completion_state {
            if completion.matches.is_empty() {
                return;
            }

            let DataCursor(cursor_x, _) = self.chat_prompt.textarea.cursor();
            let inner = chat_prompt_area.inner(Margin {
                horizontal: 1,
                vertical: 1
            });

            let cursor_screen_x = inner.x + cursor_x as u16;
            let cursor_screen_y = inner.y; 

            let popup_width = completion.matches
                .iter()
                .map(|m| m.len())
                .max()
                .unwrap_or(0) as u16
                + 2;

            let popup_height = completion.matches.len().min(8) as u16;

            let popup_area = Rect {
                x: cursor_screen_x,
                y: cursor_screen_y - popup_height,
                width: popup_width + 1,
                height: popup_height
            };

            let [list_area, scrollbar_area] = Layout::horizontal([Constraint::Min(1), Constraint::Length(1)]).areas(popup_area);

            let list_widget = List::new(completion.matches.clone())
                .style(Color::White)
                .highlight_style(Style::default()
                    .bg(Color::Yellow)
                    .fg(Color::Black));

            frame.render_widget(Clear, popup_area);

            frame.render_stateful_widget(list_widget, list_area, &mut completion.list_state);

            completion.scrollbar_state = completion.scrollbar_state
                .content_length(completion.matches.len().saturating_sub(list_area.height as usize))
                .viewport_content_length(list_area.height as usize)
                .position(completion.list_state.offset());

            frame.render_stateful_widget(
                Scrollbar::default()
                    .begin_symbol(None)
                    .end_symbol(None)
                    .track_symbol(None)
                ,scrollbar_area,
                &mut completion.scrollbar_state
            );
        }
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

    pub fn chat_page_up(&mut self) {
        self.chat_scroll.page_up(self.chat_window_area.height);
    }

    pub fn chat_page_down(&mut self) {
        self.chat_scroll.page_down(self.chat_window_area.height);
    }

    pub fn chat_auto_scroll(&mut self) {
        self.chat_scroll.auto = true;
    }
}
