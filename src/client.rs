use std::{
    error::Error,
    io::{self, stdin, Read, Write},
    net::{TcpStream, ToSocketAddrs},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
    fmt
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
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;

mod message;
use message::*;

const NAME_COLORS: &[Color] = &[
    Color::Red,
    Color::Green,
    Color::Yellow,
    Color::Blue,
    Color::Magenta,
    Color::Cyan,
    Color::LightRed,
    Color::LightGreen,
    Color::LightYellow,
    Color::LightBlue,
    Color::LightMagenta,
    Color::LightMagenta
];

fn color_index_from_name(name: &str) -> u8 {
    let mut hasher = DefaultHasher::new();
    name.to_lowercase().hash(&mut hasher);
    let hash = hasher.finish();

    let index = (hash as usize) % NAME_COLORS.len();

    index as u8
}

fn receive_messages(stream: &TcpStream, messages: Arc<Mutex<Vec<Line<'static>>>>) {
    let mut stream_clone = stream.try_clone().unwrap();
    thread::spawn(move || {
        loop {
            match receive_message(&mut stream_clone) {
                Ok(message) => {
                    let mut messages = messages.lock().unwrap();
                    match message {
                        Message::ClientConnected {timestamp_secs, client_name} => {
                            let datetime = datetime_from_timestamp(timestamp_secs).to_string();
                            let client_name_color = NAME_COLORS[color_index_from_name(&client_name) as usize];
                            // messages.push(format!("{datetime} '{client_name}' connected"));
                            messages.push(Line::from(vec![
                                datetime.into(),
                                Span::from(" "),
                                Span::styled(client_name, Style::default().fg(client_name_color)),
                                Span::from(" connected"),
                            ]));
                        }
                        Message::ClientDisconnected {timestamp_secs, client_name, reason} => {
                            let datetime = datetime_from_timestamp(timestamp_secs).to_string();
                            let client_name_color = NAME_COLORS[color_index_from_name(&client_name) as usize];
                            // messages.push(format!("{datetime} '{client_name}' disconnected (reason: {reason})"));
                            messages.push(Line::from(vec![
                                datetime.into(),
                                Span::from(" "),
                                Span::styled(client_name, Style::default().fg(client_name_color)),
                                format!(" disconnected (reason: {reason})").into()
                            ]));
                        }
                        Message::ClientMessage {timestamp_secs, client_name, msg} => {
                            let datetime = datetime_from_timestamp(timestamp_secs).to_string();
                            let client_name_color = NAME_COLORS[color_index_from_name(&client_name) as usize];
                            // messages.push(format!("{datetime} {client_name}: {msg}"));
                            messages.push(Line::from(vec![
                                datetime.into(),
                                Span::from(" "),
                                Span::styled(client_name, Style::default().fg(client_name_color)),
                                format!(": {msg}").into()
                            ]));
                        }
                        Message::ClientList { client_names } => {
                            messages.push(Line::from("Connected clients:"));

                            for client_name in client_names {
                                let client_name_color = NAME_COLORS[color_index_from_name(&client_name) as usize];
                                messages.push(Line::styled(format!("- {client_name}"), Style::default().fg(client_name_color)));
                            }
                        }
                        _ => {}
                    }
                },
                Err(ref err)
                    if err.kind() == io::ErrorKind::WouldBlock
                        || err.kind() == io::ErrorKind::TimedOut => continue,
                Err(err) => {
                    let mut messages = messages.lock().unwrap();
                    match err.kind() {
                        io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset => {
                            messages.push(format!("INFO: Server closed the connection").into());
                        }
                        _ => messages.push(format!("ERROR: {err}").into())
                    }
                    break;
                }
            }
        }
    });
}

struct App {
    exit: bool,
    input_box: TextArea<'static>,
    messages: Arc<Mutex<Vec<Line<'static>>>>,
    vertical_scroll_state: ScrollbarState,
    vertical_scroll: usize,
    last_tick: Instant,
    max_scroll: usize,
    auto_scroll: bool,
    client_name: String
}

impl App {
    const TICK_RATE: Duration = Duration::from_millis(250);

    fn new(client_name: &str) -> Self {
        Self {
            exit: false,
            input_box: TextArea::default(),
            messages: Arc::new(Mutex::new(
                vec![
                    Line::from(vec![
                        "Welcome to ".into(),
                        "ChaTTY".yellow(),
                        "!".into()
                    ]),
                    Line::from(vec![
                        "Use ".into(),
                        "UP".yellow(),
                        "/".into(),
                        "DOWN".yellow(),
                        " to scroll".into()
                    ]),
                    Line::from(vec![
                        "Type and press ".into(),
                        "ENTER".yellow(),
                        " to send".into()
                    ]),
                    Line::from(vec![
                        "Press ".into(),
                        "ESC".yellow(),
                        " to exit".into()
                    ])
                ]
            )),
            vertical_scroll_state: ScrollbarState::default(),
            vertical_scroll: 0,
            last_tick: Instant::now(),
            max_scroll: 0,
            auto_scroll: true,
            client_name: client_name.to_string()
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
                            if line_trimmed.starts_with("/") {
                                let cmd = &line_trimmed[1..];

                                match cmd {
                                    "clear" => messages.clear(),
                                    "list" => {
                                        if let Err(err) = send_message(stream, Message::GetClientList) {
                                            messages.push(format!("list: failed to get client list: {err}").into());
                                        }
                                    }
                                    unknown_cmd => messages.push(format!("CMD: Unknown command {unknown_cmd}").into())
                                }

                                self.input_box.select_all();
                                self.input_box.cut();

                                return Ok(());
                            }

                            let timestamp = chrono::Local::now().timestamp();
                            let mut message = Message::ClientMessage {
                                timestamp_secs: timestamp,
                                client_name: String::new(),
                                msg: line_trimmed.to_string(),
                            };
                            if let Err(err) = send_message_with_timestamp(stream, &mut message, true) {
                                messages.push(format!("ERROR: Failed to send message: {err}").into());
                            } else {
                                let datetime = datetime_from_timestamp(timestamp);
                                let client_name = &self.client_name;
                                messages.push(format!("{datetime} {client_name}: {line_trimmed}").into());
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

#[derive(Debug)]
enum HandshakeError {
    IO(io::Error),
    Timeout,
    InvalidMagic
}

impl fmt::Display for HandshakeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HandshakeError::IO(err) => write!(f, "{err}"),
            HandshakeError::Timeout => write!(f, "timeout expired"),
            HandshakeError::InvalidMagic => write!(f, "server returned invalid magic bytes")
        }
    }
}

impl From<io::Error> for HandshakeError {
    fn from(err: io::Error) -> Self {
        HandshakeError::IO(err)
    }
}

impl Error for HandshakeError {}

fn init_handshake(stream: &mut TcpStream) -> Result<(), HandshakeError> {
    println!("INFO: Initializing handshake...");

    stream.write_all(b"ChaTTY\0\0").map_err(|err| {
        match err.kind() {
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => {
                eprintln!("ERROR: Handshake failed: Timeout expired");
                HandshakeError::Timeout
            }
            _ => {
                eprintln!("ERROR: Handshake failed: {err}");
                HandshakeError::IO(err)
            }
        }
    })?;

    let mut server_magic_buf = [0u8; 8];
    stream.read_exact(&mut server_magic_buf).map_err(|err| {
        match err.kind() {
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => {
                eprintln!("ERROR: Handshake failed: Timeout expired");
                HandshakeError::Timeout
            }
            _ => {
                eprintln!("ERROR: Handshake failed: {err}");
                HandshakeError::IO(err)
            }
        }
    })?;

    if server_magic_buf != *b"ChaTTY\0\0" {
        eprintln!("ERROR: Handshake failed: Not a ChaTTY server");
        return Err(HandshakeError::InvalidMagic);
    }

    Ok(())
}

fn check_for_updates() -> Result<(), Box<dyn Error>> {
    println!("INFO: Checking for updates...");

    let status = self_update::backends::github::Update::configure()
        .repo_owner("Marjan112")
        .repo_name("chatty")
        .bin_name("chatty_client")
        .current_version(env!("CARGO_PKG_VERSION"))
        .build()?
        .update()?;

    println!("{}", status.version());
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    check_for_updates().ok();

    println!("Enter the server address (ip:port)");
    let mut server_address = String::new();
    stdin().read_line(&mut server_address).unwrap();

    let server_address_trimmed = server_address.trim();

    let server_sock_addr = server_address_trimmed.to_socket_addrs().map_err(|err| {
        eprintln!("ERROR: Failed to resolve address {server_address_trimmed}");
        err
    })?.find(|a| a.is_ipv4()).unwrap();

    let mut stream = TcpStream::connect_timeout(&server_sock_addr, Duration::from_secs(20)).map_err(|err| {
        eprintln!("ERROR: Failed to connect: {err}");
        err
    })?;

    stream.set_read_timeout(Some(Duration::from_secs(20))).map_err(|err| {
        eprintln!("ERROR: Failed to set read timeout: {err}");
        err
    })?;
    stream.set_write_timeout(Some(Duration::from_secs(20))).map_err(|err| {
        eprintln!("ERROR: Failed to set write timeout: {err}");
        err
    })?;

    println!("INFO: Connected to {server_sock_addr}");

    init_handshake(&mut stream)?;

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
    send_message_with_timestamp(&mut stream, &mut connect_message, false).map_err(|err| {
        eprintln!("ERROR: Failed to send your name to the server: {err}");
        err
    })?;

    let mut terminal = ratatui::init();
    let app_result = App::new(name_trimmed).run(&mut terminal, &mut stream);
    ratatui::restore();
    app_result
}
