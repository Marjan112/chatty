use std::{
    error::Error,
    io::{self, Read, Write},
    net::TcpStream,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant}
};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Layout, Constraint, Direction, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Terminal,
    Frame
};

struct App {
    messages: Vec<String>,
    default_terminal_messages: Vec<String>, // Naming - The unsolvable computer science problem
    input: String,
    scroll_offset: usize,
    visible_height: usize,
    running: bool
}

impl App {
    fn new() -> Self {
        Self {
            messages: vec![
                String::from("Welcome to ChaTTY!"),
                String::from("Use UP/DOWN to scroll"),
                String::from("Type and press Enter to send"),
            ],
            default_terminal_messages: Vec::new(),
            input: String::new(),
            scroll_offset: 0,
            visible_height: 0,
            running: true
        }
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
            .block(Block::default().title("ChaTTY").borders(Borders::ALL))
            .style(Style::default().fg(Color::White));

        frame.render_widget(chat_box, area);
    }

    fn draw_input_box(&self, frame: &mut Frame, area: Rect) {
        let input = Paragraph::new(&*self.input)
            .block(Block::default().borders(Borders::ALL).title("You:"))
            .style(Style::default().fg(Color::Yellow));

        frame.render_widget(input, area);
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

fn receive_messages(stream: &TcpStream, app: Arc<Mutex<App>>) -> Result<thread::JoinHandle<()>, Box<dyn Error>> {
    let mut stream_clone = stream.try_clone()?;
    stream_clone.set_read_timeout(Some(Duration::from_millis(200))).unwrap();

    let join_handle = thread::spawn(move || {
        let mut buf = [0u8; 128];
        loop {
            let running = {
                let app = app.lock().unwrap();
                app.running
            };

            if !running {
                break;
            }

            match stream_clone.read(&mut buf) {
                Ok(0) => {
                    let mut app = app.lock().unwrap();
                    app.default_terminal_messages.push(String::from("[INFO]: Server closed connection"));
                    app.running = false;
                }
                Ok(n) => {
                    if let Ok(msg) = str::from_utf8(&buf[..n]) {
                        let lines: Vec<String> = msg
                            .lines()
                            .map(|line| line.trim().to_string())
                            .filter(|line| !line.is_empty())
                            .collect();

                        if !lines.is_empty() {
                            let mut app = app.lock().unwrap();
                            app.messages.extend(lines);
                        }
                    }
                }
                Err(ref err)
                    if err.kind() == io::ErrorKind::WouldBlock ||
                        err.kind() == io::ErrorKind::TimedOut => continue,
                Err(err) => {
                    let mut app = app.lock().unwrap();
                    app.default_terminal_messages.push(format!("[ERROR]: {err}"));
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
    io::stdin().read_line(&mut input_server_address).map_err(|err| {
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
    io::stdin().read_line(&mut input_name).map_err(|err| {
        eprintln!("[ERROR]: Failed to read name: {err}");
        err
    })?;
    let name = input_name.trim();
    if name.len() < 4 || name.len() > 20 {
        eprintln!("[ERROR]: Invalid name length");
        return Ok(());
    }

    stream.write(format!("{name}\n").as_bytes()).map_err(|err| {
        eprintln!("[ERROR]: Failed to send your name to the server: {err}");
        err
    })?;

    let tick_rate = Duration::from_millis(200);
    let mut last_tick = Instant::now();

    let app = Arc::new(Mutex::new(App::new()));
    let join_handle = receive_messages(&stream, app.clone())?;

    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    while app.lock().unwrap().running {
        terminal.draw(|frame| app.lock().unwrap().draw_ui(frame))?;

        let timeout = tick_rate.checked_sub(last_tick.elapsed()).unwrap_or(Duration::from_secs(0));

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    let mut app = app.lock().unwrap();
                    match key.code {
                        KeyCode::Char(c) => app.input.push(c),
                        KeyCode::Backspace => {
                            app.input.pop();
                        }
                        KeyCode::Enter => {
                            let msg = app.input.trim().to_string();
                            if !msg.is_empty() {
                                if let Err(err) = stream.write(format!("{msg}\n").as_bytes()) {
                                    app.messages.push(format!("[ERROR]: Could not send message: {err}"));
                                } else {
                                    app.messages.push(format!("You: {msg}"));
                                    app.input.clear();
                                }
                            }
                        }
                        KeyCode::Up => app.scroll_up(),
                        KeyCode::Down => app.scroll_down(),
                        KeyCode::Esc => app.running = false,
                        _ => {}
                    }
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
