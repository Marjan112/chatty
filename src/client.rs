use std::{
    error::Error,
    io::{self, stdin},
    net::TcpStream,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use ratatui::{
    crossterm::event::{self, Event},
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
    DefaultTerminal,
    Frame,
};
use tui_textarea::{Input, Key, TextArea};

mod message;
use message::*;

fn receive_messages(stream: &TcpStream, messages: Arc<Mutex<Vec<String>>>) {
    let mut stream_clone = stream.try_clone().unwrap();
    thread::spawn(move || {
        loop {
            match receive_message(&mut stream_clone) {
                Ok(message) => {
                    let mut messages = messages.lock().unwrap();
                    match message {
                        Message::ClientConnected {timestamp_secs, client_name} => {
                            let datetime = datetime_from_timestamp(timestamp_secs);
                            messages.push(format!("{datetime} '{client_name}' connected"));
                        }
                        Message::ClientDisconnected {timestamp_secs, client_name, reason} => {
                            let datetime = datetime_from_timestamp(timestamp_secs);
                            messages.push(format!("{datetime} '{client_name}' disconnected (reason: {reason})"));
                        }
                        Message::ClientMessage {timestamp_secs, client_name, msg} => {
                            let datetime = datetime_from_timestamp(timestamp_secs);
                            messages.push(format!("{datetime} {client_name}: {msg}"));
                        }
                        _ => {}
                    }
                },
                Err(ref err)
                    if err.kind() == io::ErrorKind::WouldBlock
                        || err.kind() == io::ErrorKind::TimedOut => continue,
                Err(err) => {
                    let mut messages = messages.lock().unwrap();
                    if err.kind() == io::ErrorKind::UnexpectedEof {
                        messages.push(String::from("INFO: Server closed the connection"));
                    } else {
                        messages.push(format!("ERROR: {err}"));
                    }
                    break;
                }
            }
        }
    });
}

struct App<'a> {
    exit: bool,
    input_box: TextArea<'a>,
    messages: Arc<Mutex<Vec<String>>>,
    vertical_scroll_state: ScrollbarState,
    vertical_scroll: usize,
    last_tick: Instant,
    max_scroll: usize,
    auto_scroll: bool,
}

impl<'a> App<'a> {
    const TICK_RATE: Duration = Duration::from_millis(250);

    fn new() -> Self {
        Self {
            exit: false,
            input_box: TextArea::default(),
            messages: Arc::new(Mutex::new(
                vec![
                    String::from("Welcome to ChaTTY!"),
                    String::from("Use UP/DOWN to scroll"),
                    String::from("Type and press Enter to send"),
                    String::from("Press ESC to exit")
                ]
            )),
            vertical_scroll_state: ScrollbarState::default(),
            vertical_scroll: 0,
            last_tick: Instant::now(),
            max_scroll: 0,
            auto_scroll: true,
        }
    }

    fn handle_events(&mut self, stream: &mut TcpStream) -> Result<(), Box<dyn Error>> {
        let timeout = Self::TICK_RATE.saturating_sub(self.last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                let input = Input::from(key);
                match input.key {
                    Key::Esc => self.exit = true,
                    Key::Enter => {
                        let mut messages = self.messages.lock().unwrap();
                        let line = self.input_box.lines().join("\n");
                        let line_trimmed = line.trim();
                        if !line_trimmed.is_empty() {
                            let timestamp = chrono::Local::now().timestamp();
                            let mut message = Message::ClientMessage {
                                timestamp_secs: timestamp,
                                client_name: String::new(),
                                msg: line_trimmed.to_string(),
                            };
                            if let Err(err) = send_message(stream, &mut message, true) {
                                messages.push(format!("ERROR: Failed to send message: {err}"))
                            } else {
                                let datetime = datetime_from_timestamp(timestamp);
                                messages.push(format!("{datetime} You: {line_trimmed}"));
                            }
                        }
                        self.input_box.select_all();
                        self.input_box.cut();
                    }
                    Key::Up => self.vertical_scroll_up(),
                    Key::Down => self.vertical_scroll_down(),
                    _ => {
                        self.input_box.input(input);
                    }
                }
            }
        }
        Ok(())
    }

    fn run(&mut self, terminal: &mut DefaultTerminal, stream: &mut TcpStream) -> Result<(), Box<dyn Error>> {
        receive_messages(stream, self.messages.clone());

        while !self.exit {
            terminal.draw(|frame| self.draw_ui(frame))?;
            self.handle_events(stream)?;

            if self.last_tick.elapsed() >= Self::TICK_RATE {
                self.last_tick = Instant::now();
            }
        }
        Ok(())
    }

    fn draw_ui(&mut self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(3)
            ])
            .split(frame.area());

        self.draw_chat_window(frame, chunks[0]);
        self.draw_input_box(frame, chunks[1]);
    }

    fn draw_chat_window(&mut self, frame: &mut Frame, chat_window_area: Rect) {
        let messages = self.messages.lock().unwrap();

        let lines: Vec<Line> = messages
            .iter()
            .map(|msg| Line::from(Span::raw(msg)))
            .collect();

        let block = Block::default()
            .title(" ChaTTY ".yellow())
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL);

        let chat = Paragraph::new(lines)
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

    fn draw_input_box(&mut self, frame: &mut Frame, input_box_area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" You: ".white())
            .fg(Color::Yellow);

        self.input_box.set_block(block);
        self.input_box.set_cursor_line_style(Style::default());
        self.input_box.set_placeholder_text("Your message...");

        frame.render_widget(&self.input_box, input_box_area);
    }

    fn vertical_scroll_up(&mut self) {
        self.vertical_scroll = self.vertical_scroll.saturating_sub(1);
        self.vertical_scroll_state = self.vertical_scroll_state.position(self.vertical_scroll);
        self.auto_scroll = false;
    }

    fn vertical_scroll_down(&mut self) {
        self.vertical_scroll = self.vertical_scroll.saturating_add(1);
        self.vertical_scroll_state = self.vertical_scroll_state.position(self.vertical_scroll);
        if self.vertical_scroll >= self.max_scroll {
            self.auto_scroll = true;
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    println!("Enter the server address (ip:port)");
    let mut server_address = String::new();
    stdin().read_line(&mut server_address).unwrap();
    let mut stream = TcpStream::connect(server_address.trim()).map_err(|err| {
        eprintln!("ERROR: Failed to connect: {err}");
        err
    })?;

    let mut handshake_message = Message::Handshake {
        // This magic number 1415669827 is array ['C', 'h', 'a', 'T', 'T', 'Y', 0, 0] interpreted as a number
        magic: 1415669827
    };
    send_message(&mut stream, &mut handshake_message, false).map_err(|err| {
        eprintln!("Failed to perform a handshake: {err}");
        err
    })?;

    println!("Enter your name:");
    let mut name = String::new();
    stdin().read_line(&mut name).unwrap();
    let name_trimmed = name.trim();
    if name_trimmed.is_empty() {
        eprintln!("ERROR: Cant have an empty name mate");
        return Ok(());
    }

    let mut connect_message = Message::ClientConnected {
        timestamp_secs: 0,
        client_name: name_trimmed.to_string(),
    };
    send_message(&mut stream, &mut connect_message, false).map_err(|err| {
        eprintln!("ERROR: Failed to send your name to the server: {err}");
        err
    })?;

    let mut terminal = ratatui::init();
    let app_result = App::new().run(&mut terminal, &mut stream);
    ratatui::restore();
    app_result
}
