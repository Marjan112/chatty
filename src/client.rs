use std::{
    error::Error,
    io::{self, stdin, Read, Write},
    net::{TcpStream, ToSocketAddrs},
    sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}},
    thread,
    time::{Duration, Instant},
    fmt,
    hash::{Hash, Hasher},
    collections::hash_map::DefaultHasher
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

mod env;
use env::*;

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
    Color::LightMagenta
];

fn color_index_from_name(name: &str) -> usize {
    let mut hasher = DefaultHasher::new();
    name.to_lowercase().hash(&mut hasher);
    let hash = hasher.finish();

    (hash as usize) % NAME_COLORS.len()
}

fn receive_message(stream: &mut TcpStream) -> io::Result<(i64, Message)> {
    let mut timestamp_buf = [0u8; 8];
    stream.read_exact(&mut timestamp_buf)?;
    let timestamp = i64::from_le_bytes(timestamp_buf);

    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = u32::from_le_bytes(len_buf);

    let mut buf = vec![0u8; len as usize];
    stream.read_exact(&mut buf)?;

    let decoded =
        bincode::decode_from_slice(&buf, bincode::config::standard())
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?
        .0;

    Ok((timestamp, decoded))
}

fn receive_messages(mut stream: TcpStream, shared: Arc<Shared>) {
    thread::spawn(move || {
        while !shared.exit.load(Ordering::SeqCst) {
            match receive_message(&mut stream) {
                Ok((timestamp_secs, message)) => {
                    let mut messages = shared.messages.lock().unwrap();
                    let datetime = datetime_from_timestamp(timestamp_secs).to_string();
                    match message {
                        Message::ClientConnected {client_name} => {
                            let client_name_color = NAME_COLORS[color_index_from_name(&client_name)];
                            messages.push(Line::from(vec![
                                datetime.into(),
                                Span::from(" "),
                                Span::styled(client_name, Style::default().fg(client_name_color)),
                                Span::from(" connected"),
                            ]));
                        }
                        Message::ClientDisconnected {client_name, reason} => {
                            let client_name_color = NAME_COLORS[color_index_from_name(&client_name)];
                            messages.push(Line::from(vec![
                                datetime.into(),
                                Span::from(" "),
                                Span::styled(client_name, Style::default().fg(client_name_color)),
                                format!(" disconnected (reason: {reason})").into()
                            ]));
                        }
                        Message::ClientMessage {client_name, msg} => {
                            let client_name_color = NAME_COLORS[color_index_from_name(&client_name)];
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
                                let client_name_color = NAME_COLORS[color_index_from_name(&client_name)];
                                messages.push(Line::styled(format!("- {client_name}"), Style::default().fg(client_name_color)));
                            }
                        }
                        Message::ClientKicked {client_name, reason} => {
                            if client_name == shared.name {
                                shared.after_disconnect_messages
                                    .lock()
                                    .unwrap()
                                    .push(format!("INFO: You are kicked from the server (reason: {reason})"));

                                shared.exit.store(true, Ordering::SeqCst);
                            }
                        }
                        _ => {}
                    }
                },
                Err(ref err)
                    if err.kind() == io::ErrorKind::WouldBlock
                        || err.kind() == io::ErrorKind::TimedOut => continue,
                Err(err) => {
                    let mut after_disconnect_messages = shared.after_disconnect_messages.lock().unwrap();

                    match err.kind() {
                        io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset => {
                            after_disconnect_messages.push(String::from("INFO: Server closed the connection"));
                        }
                        _ => after_disconnect_messages.push(format!("ERROR: {err}"))
                    }

                    shared.exit.store(true, Ordering::SeqCst);
                }
            }
        }
    });
}

fn clear_command(app: &mut App) {
    let mut messages = app.shared.messages.lock().unwrap();
    messages.clear();
}

fn list_command(app: &mut App) {
    let mut messages = app.shared.messages.lock().unwrap();
    if let Err(err) = send_message(&mut app.stream, Message::GetClientList, None) {
        messages.push(format!("list: failed to get client list: {err}").into());
    }
}

fn help_command(app: &mut App) {
    let mut messages = app.shared.messages.lock().unwrap();

    messages.push(Line::from("Help:"));

    for Command {description, signature, ..} in COMMANDS {
        messages.push(format!("- {signature} - {description}").into());
    }
}

struct Command {
    name: &'static str,
    description: &'static str,
    signature: &'static str,
    run: fn(&mut App)
}

const COMMANDS: &[Command] = &[
    Command {
        name: "clear",
        description: "Clears the chat",
        signature: "/clear",
        run: clear_command
    },
    Command {
        name: "list",
        description: "Lists the connected clients",
        signature: "/list",
        run: list_command
    },
    Command {
        name: "help",
        description: "Helps, duh",
        signature: "/help",
        run: help_command
    }
];

fn find_command(name: &str) -> Option<&Command> {
    COMMANDS
        .iter()
        .find(|command| command.name == name)
}

struct Shared {
    messages: Mutex<Vec<Line<'static>>>,
    after_disconnect_messages: Mutex<Vec<String>>,
    exit: AtomicBool,
    name: String
}

impl Shared {
    fn new(name: String) -> Self {
        Self {
            messages: Mutex::new(
                vec![
                    Line::from(vec![
                        "Welcome to ".into(),
                        "ChaTTY ".yellow(),
                        CHATTY_VERSION.yellow(),
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
            ),
            after_disconnect_messages: Mutex::new(Vec::new()),
            exit: AtomicBool::new(false),
            name
        }
    }
}

struct App {
    input_box: TextArea<'static>,
    vertical_scroll_state: ScrollbarState,
    vertical_scroll: usize,
    last_tick: Instant,
    max_scroll: usize,
    auto_scroll: bool,
    stream: TcpStream,
    shared: Arc<Shared>
}

impl App {
    const TICK_RATE: Duration = Duration::from_millis(250);

    fn new(name: &str, stream: TcpStream) -> Self {
        Self {
            input_box: TextArea::default(),
            vertical_scroll_state: ScrollbarState::default(),
            vertical_scroll: 0,
            last_tick: Instant::now(),
            max_scroll: 0,
            auto_scroll: true,
            stream,
            shared: Arc::new(Shared::new(name.to_string()))
        }
    }

    fn handle_events(&mut self) -> Result<(), Box<dyn Error>> {
        let timeout = Self::TICK_RATE.saturating_sub(self.last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                let input = Input::from(key);
                match input.key {
                    Key::Esc => self.shared.exit.store(true, Ordering::SeqCst),
                    Key::Enter => {
                        let input = self.input_box.lines().join("\n");
                        let line = input.trim();

                        if line.is_empty() {
                            return Ok(());
                        }

                        if let Some(cmd_name) = line.strip_prefix("/") {
                            if let Some(cmd) = find_command(cmd_name) {
                                (cmd.run)(self);
                            } else {
                                let mut messages = self.shared.messages.lock().unwrap();
                                messages.push(format!("CMD: Unknown command {cmd_name}").into());
                            }

                            self.input_box.select_all();
                            self.input_box.cut();

                            return Ok(());
                        }

                        let message = Message::ClientMessage {
                            client_name: String::new(),
                            msg: line.to_string(),
                        };

                        let timestamp_secs = chrono::Local::now().timestamp();

                        let mut messages = self.shared.messages.lock().unwrap();

                        if let Err(err) = send_message(&mut self.stream, message, Some(timestamp_secs)) {
                            messages.push(format!("ERROR: Failed to send message: {err}").into());
                        } else {
                            let datetime = datetime_from_timestamp(timestamp_secs);
                            let client_name = &self.shared.name;
                            messages.push(format!("{datetime} {client_name}: {line}").into());
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

    fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<(), Box<dyn Error>> {
        receive_messages(self.stream.try_clone()?, self.shared.clone());

        while !self.shared.exit.load(Ordering::SeqCst) {
            terminal.draw(|frame| self.draw_ui(frame))?;
            self.handle_events()?;

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
        let messages = self.shared.messages.lock().unwrap();

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
            .title(" You: ".reset())
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
            HandshakeError::Timeout => write!(f, "Timeout expired"),
            HandshakeError::InvalidMagic => write!(f, "Not a ChaTTY server")
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
    stream.write_all(b"ChaTTY\0\0").map_err(|err| {
        match err.kind() {
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => HandshakeError::Timeout,
            _ => HandshakeError::IO(err)
        }
    })?;

    let mut server_magic_buf = [0u8; 8];
    stream.read_exact(&mut server_magic_buf).map_err(|err| {
        match err.kind() {
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => HandshakeError::Timeout,
            _ => HandshakeError::IO(err)
        }
    })?;

    if server_magic_buf != *b"ChaTTY\0\0" {
        return Err(HandshakeError::InvalidMagic);
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    println!("INFO: ChaTTY {CHATTY_VERSION}");

    println!("Enter the server address (ip:port)");
    let mut server_address = String::new();
    stdin().read_line(&mut server_address).unwrap();

    let server_address_trimmed = server_address.trim();

    let server_sock_addr = server_address_trimmed.to_socket_addrs().inspect_err(|err| {
        eprintln!("ERROR: Failed to resolve address {server_address_trimmed}: {err}");
    })?.find(|a| a.is_ipv4()).unwrap();

    let mut stream = TcpStream::connect_timeout(&server_sock_addr, Duration::from_secs(20)).map_err(|err| {
        eprintln!("ERROR: Failed to connect: {err}");
        err
    })?;

    stream.set_read_timeout(Some(Duration::from_secs(20))).inspect_err(|err| {
        eprintln!("ERROR: Failed to set read timeout: {err}");
    })?;
    stream.set_write_timeout(Some(Duration::from_secs(20))).inspect_err(|err| {
        eprintln!("ERROR: Failed to set write timeout: {err}");
    })?;

    println!("INFO: Connected to {server_sock_addr}");
    println!("INFO: Initiating a handshake...");
    init_handshake(&mut stream).inspect_err(|err| {
        eprintln!("ERROR: Handshake failed: {err}");
    })?;

    println!("Enter your name:");
    let mut name = String::new();
    stdin().read_line(&mut name).unwrap();
    let name_trimmed = name.trim();
    if name_trimmed.is_empty() {
        eprintln!("ERROR: Cant have an empty name mate");
        return Ok(());
    }

    let connect_message = Message::ClientConnected {
        client_name: name_trimmed.to_string(),
    };
    send_message(&mut stream, connect_message, None).inspect_err(|err| {
        eprintln!("ERROR: Failed to send your name to the server: {err}");
    })?;

    let mut terminal = ratatui::init();
    let mut app = App::new(name_trimmed, stream);
    let app_result = app.run(&mut terminal);

    ratatui::restore();

    for msg in app.shared.after_disconnect_messages.lock().unwrap().iter() {
        println!("{msg}");
    }

    app_result
}
