use std::{
    error::Error,
    io::{self, Read, Write},
    net::TcpStream,
    sync::{Arc, Mutex},
    thread,
    time::Duration
};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Layout, Constraint, Direction},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal
};

struct App {
    messages: Vec<String>,
    default_terminal_messages: Vec<String>, // Naming - The unsolvable computer science problem
    input: String,
    running: bool
}

impl App {
    fn new() -> Self {
        Self {
            messages: Vec::new(),
            default_terminal_messages: Vec::new(),
            input: String::new(),
            running: true
        }
    }
}

fn receive_messages(stream: &TcpStream, app: Arc<Mutex<App>>) -> Result<(), Box<dyn Error>> {
    let mut stream_clone = stream.try_clone()?;
    thread::spawn(move || {
        let mut buf = [0u8; 128];
        loop {
            match stream_clone.read(&mut buf) {
                Ok(0) => {
                    let mut app = app.lock().unwrap();
                    app.default_terminal_messages.push("[INFO]: Server closed connection".to_string());
                    app.running = false;
                    break;
                }
                Ok(n) => {
                    if let Ok(msg) = str::from_utf8(&buf[..n]) {
                        let mut app = app.lock().unwrap();
                        for line in msg.split('\n') {
                            if !line.trim().is_empty() {
                                app.messages.push(line.trim().to_string());
                            }
                        }
                    }
                }
                Err(_) => {
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }
    });

    Ok(())
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

    let app = Arc::new(Mutex::new(App::new()));
    receive_messages(&stream, app.clone())?;

    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    while app.lock().unwrap().running {
        terminal.draw(|frame| {
            let area = frame.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(3)].as_ref())
                .split(area);

            let app = app.lock().unwrap();

            let messages: Vec<Line> = app.messages
                .iter()
                .rev()
                .take((chunks[0].height as usize) - 2)
                .rev()
                .map(|m| Line::from(Span::raw(m.clone())))
                .collect();

            let chat = Paragraph::new(messages)
                .block(Block::default().title("Chat").borders(Borders::ALL))
                .wrap(Wrap { trim: true });
            frame.render_widget(chat, chunks[0]);

            let input = Paragraph::new(app.input.clone())
                .style(Style::default().fg(Color::Yellow))
                .block(Block::default().borders(Borders::ALL).title("You:"));
            frame.render_widget(input, chunks[1]);
        })?;

        if event::poll(Duration::from_millis(100))? {
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
                        KeyCode::Esc => app.running = false,
                        _ => {}
                    }
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    for msg in &app.lock().unwrap().default_terminal_messages {
        println!("{msg}\n");
    }

    Ok(())
}
