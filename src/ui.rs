use ratatui::{
    layout::{Layout, Direction, Alignment, Constraint, Rect},
    widgets::{Block, Borders, Paragraph, Wrap, Scrollbar, ScrollbarState, ScrollbarOrientation},
    style::{Style, Stylize, Color},
    text::Span,
    Frame
};
use tui_textarea::TextArea;

use crate::Shared;

pub struct Ui {
    pub input_box: TextArea<'static>,
    vertical_scroll_state: ScrollbarState,
    vertical_scroll: usize,
    max_scroll: usize,
    auto_scroll: bool
}

impl Ui {
    pub fn new() -> Self {
        Self {
            input_box: TextArea::default(),
            vertical_scroll_state: ScrollbarState::default(),
            vertical_scroll: 0,
            max_scroll: 0,
            auto_scroll: true
        }
    }

    pub fn draw(&mut self, frame: &mut Frame, shared: &Shared) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([Constraint::Min(1), Constraint::Length(3)])
            .split(frame.area());

        self.draw_chat_window(frame, chunks[0], shared);
        self.draw_input_box(frame, chunks[1], shared);
    }

    pub fn draw_chat_window(&mut self, frame: &mut Frame, chat_window_area: Rect, shared: &Shared) {
        let messages = shared.messages.lock().unwrap();

        let block = Block::default()
            .title(" ChaTTY ".yellow())
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL);

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

    pub fn draw_input_box(&mut self, frame: &mut Frame, input_box_area: Rect, shared: &Shared) {
        let name = shared.name.lock().unwrap().to_owned();
        let color: Color = shared.color.lock().unwrap().to_owned().into();

        let block = Block::default()
            .borders(Borders::ALL)
            .title(vec![" You (".into(), Span::styled(name, Style::default().fg(color)), "): ".into()])
            .fg(Color::Yellow);

        self.input_box.set_block(block);
        self.input_box.set_cursor_line_style(Style::default().fg(Color::Reset));
        self.input_box.set_placeholder_text("Your message...");

        frame.render_widget(&self.input_box, input_box_area);
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
