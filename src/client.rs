use std::{
    error::Error,
    io::{ErrorKind, stdin, stdout},
    net::TcpStream,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant}
};
use ratatui::{
    prelude::*,
    crossterm::{
        event::{self, Event},
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    },
    backend::CrosstermBackend,
    layout::{Layout, Constraint, Direction, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
    Frame
};
use tui_textarea::{Input, Key, TextArea};
use chrono::Local;

mod message;
use message::{Message, receive_message, send_message, datetime_from_timestamp};

const MAX_CHARS: usize = 128;

struct App<'a> {
    messages: Vec<String>,
    default_terminal_messages: Vec<String>, // Naming - The unsolvable computer science problem
    textarea: TextArea<'a>,
    scroll_offset: usize,
    visible_height: usize,
    running: bool
}

impl<'a> App<'a> {
    fn new() -> Self {
        Self {
            messages: vec![
                String::from("Welcome to ChaTTY!"),
                String::from("Use UP/DOWN to scroll"),
                String::from("Type and press Enter to send"),
                String::from("Press ESC to exit")
            ],
            default_terminal_messages: Vec::new(),
            textarea: TextArea::default(),
            scroll_offset: 0,
            visible_height: 0,
            running: true
        }
    }

    fn get_textarea_count(&self) -> usize {
        self.textarea
            .lines()
            .iter()
            .map(|line| line.bytes().count())
            .sum()
    }

    fn draw_chat_window(&mut self, frame: &mut Frame, area: Rect) {
        self.visible_height = (area.height as usize).saturating_sub(2);

        let height = area.height as usize - 2;
        let total = self.messages.len();

        let start = total.saturating_sub(height + self.scroll_offset);
        let end = total.saturating_sub(self.scroll_offset);
        let visible_msgs = &self.messages[start..end.min(total)];

        let lines: Vec<Line> = visible_msgs
            .iter()
            .map(|msg| Line::from(Span::raw(msg)))
            .collect();

        let chat_box = Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(Block::default().title("ChaTTY").borders(Borders::ALL))
            .style(Style::default().fg(Color::White));

        frame.render_widget(chat_box, area);
    }

    fn draw_input_box(&mut self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" You: ({}/{}) ", self.get_textarea_count(), MAX_CHARS))
            .fg(Color::Yellow);

        self.textarea.set_block(block);
        self.textarea.set_cursor_line_style(Style::default());
        self.textarea.set_placeholder_text("Your message...");
        frame.render_widget(&self.textarea, area);
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

    fn scroll_up(&mut self) {
        let max_scroll = self.messages.len().saturating_sub(self.visible_height);
        if self.scroll_offset < max_scroll {
            self.scroll_offset += 1;
        }
    }

    fn scroll_down(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
        }
    }
}

fn receive_messages(stream: &TcpStream, app: Arc<Mutex<App<'static>>>) -> Result<thread::JoinHandle<()>, Box<dyn Error>> {
    let mut stream_clone = stream.try_clone()?;
    stream_clone.set_read_timeout(Some(Duration::from_millis(200))).unwrap();

    let join_handle = thread::spawn(move || {
        loop {
            let running = {
                let app = app.lock().unwrap();
                app.running
            };

            if !running {
                break;
            }

            match receive_message(&mut stream_clone) {
                Ok(message) => {
                    let mut app = app.lock().unwrap();
                    match message {
                        Message::ClientConnected { timestamp_secs, client_name } => {
                            let datetime = datetime_from_timestamp(timestamp_secs);
                            app.messages.push(format!("{datetime} '{client_name}' connected"));
                        }
                        Message::ClientDisconnected { timestamp_secs, client_name, reason } => {
                            let datetime = datetime_from_timestamp(timestamp_secs);
                            app.messages.push(format!("{datetime} '{client_name}' disconnected (reason: {reason})"));
                        }
                        Message::ClientMessage { timestamp_secs, client_name, msg } => {
                            let datetime = datetime_from_timestamp(timestamp_secs);
                            app.messages.push(format!("{datetime} {client_name}: {msg}"));
                        }
                    }
                }
                Err(ref err)
                    if err.kind() == ErrorKind::WouldBlock ||
                        err.kind() == ErrorKind::TimedOut => continue,
                Err(err) => {
                    let mut app = app.lock().unwrap();
                    app.default_terminal_messages.push(format!("[INFO]: Server closed connection: {err}"));
                    app.running = false;
                }
            }
        }
    });

    Ok(join_handle)
}

fn main() -> Result<(), Box<dyn Error>> {
    println!("Enter the server address (ip:port):");
    let mut input_server_address = String::new();
    stdin().read_line(&mut input_server_address).map_err(|err| {
        eprintln!("[ERROR]: Failed to read server address: {err}");
        err
    })?;
    let server_address = input_server_address.trim();

    let mut stream = TcpStream::connect(server_address).map_err(|err| {
        eprintln!("[ERROR]: Failed to connect: {err}");
        err
    })?;

    println!("Connected");
    println!("Enter your name (4-20):");
    let mut input_name = String::new();
    stdin().read_line(&mut input_name).map_err(|err| {
        eprintln!("[ERROR]: Failed to read name: {err}");
        err
    })?;
    let name = input_name.trim();
    if name.len() < 4 || name.len() > 20 {
        eprintln!("[ERROR]: Invalid name length");
        return Ok(());
    }

    let mut connect_message = Message::ClientConnected {
        timestamp_secs: 0,
        client_name: name.to_string()
    };
    send_message(&mut stream, &mut connect_message, false).map_err(|err| {
        eprintln!("[ERROR]: Failed to send your name to the server: {err}");
        err
    })?;

    let tick_rate = Duration::from_millis(200);
    let mut last_tick = Instant::now();

    let app: Arc<Mutex<App<'static>>> = Arc::new(Mutex::new(App::new()));
    let join_handle = receive_messages(&stream, app.clone())?;

    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    while app.lock().unwrap().running {
        terminal.draw(|frame| app.lock().unwrap().draw_ui(frame))?;

        let timeout = tick_rate.checked_sub(last_tick.elapsed()).unwrap_or(Duration::from_secs(0));

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                let mut app = app.lock().unwrap();
                let input = Input::from(key);
                match input.key {
                    Key::Esc => app.running = false,
                    Key::Enter => {
                        let msg = app.textarea.lines().join("\n");
                        let msg_trimmed = msg.trim();
                        if !msg_trimmed.is_empty() {
                            let mut message = Message::ClientMessage {
                                timestamp_secs: Local::now().timestamp(),
                                client_name: name.to_string(),
                                msg: msg_trimmed.to_string()
                            };
                            match send_message(&mut stream, &mut message, true) {
                                Ok(_) => {
                                    if let Message::ClientMessage { timestamp_secs, msg, .. } = message {
                                        let datetime = datetime_from_timestamp(timestamp_secs);
                                        app.messages.push(format!("{datetime} You: {msg}"));
                                    }
                                }
                                Err(err) => app.messages.push(format!("[ERROR]: Failed to send message: {err}"))
                            }
                        }
                        app.textarea.select_all();
                        app.textarea.cut();
                    },
                    Key::Up => app.scroll_up(),
                    Key::Down => app.scroll_down(),
                    Key::Char(_) if app.get_textarea_count() < MAX_CHARS => {
                        app.textarea.input(input);
                    }
                    Key::Left
                    | Key::Right
                    | Key::Backspace
                    | Key::Delete
                    | Key::Home
                    | Key::End => {
                        app.textarea.input(input);
                    }
                    _ => {}
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    for msg in &app.lock().unwrap().default_terminal_messages {
        println!("{msg}");
    }

    join_handle.join().unwrap();

    Ok(())
}
