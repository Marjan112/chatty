use std::{
    error::Error,
    io::{self, stdin, Read, Write},
    net::{TcpStream, ToSocketAddrs},
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};
use ratatui::{
    crossterm::event::{self, Event},
    style::{Color, Style},
    text::{Line, Span},
    DefaultTerminal,
};
use tui_textarea::{Input, Key};

mod chat_color;
use chat_color::*;

mod message;
use message::*;

mod net;
use net::*;

mod shared;
use shared::*;

mod env;
use env::*;

mod ui;
use ui::Ui;

mod handshake_error;
use handshake_error::HandshakeError;

fn exit_app(app: &mut App, _: &str) {
    app.shared.exit.store(true, Ordering::SeqCst);
}

struct Command {
    name: &'static str,
    description: &'static str,
    signature: &'static str,
    run: fn(&mut App, &str)
}

const COMMANDS: &[Command] = &[
    Command {
        name: "help",
        description: "Helps, duh",
        signature: "/help",
        run: |app, _| {
            let mut messages = app.shared.messages.lock().unwrap();

            messages.push(Line::from("Help:"));

            for Command {description, signature, ..} in COMMANDS {
                messages.push(format!("• {signature} - {description}").into());
            }
        }
    },
    Command {
        name: "clear",
        description: "Clears the chat",
        signature: "/clear",
        run: |app, _| app.shared.messages.lock().unwrap().clear()
    },
    Command {
        name: "list",
        description: "Lists the connected clients",
        signature: "/list",
        run: |app, _| {
            let mut messages = app.shared.messages.lock().unwrap();
            if let Err(err) = send_message(&mut app.stream, Message::GetClientList, None) {
                messages.push(format!("list: failed to get client list: {err}").into());
            }
        }
    },
    Command {
        name: "name",
        description: "Change your display name",
        signature: "/name <new name>",
        run: |app, new_name| {
            let mut messages = app.shared.messages.lock().unwrap();
            let mut current_name = app.shared.name.lock().unwrap();

            if new_name.is_empty() {
                messages.push("usage: /name <new name>".into());
                return;
            }

            if new_name == *current_name {
                messages.push(format!("name: your display name is already '{new_name}'").into());
                return;
            }

            let old_name = current_name.clone();

            *current_name = new_name.to_string();

            if let Err(err) = send_message(&mut app.stream, Message::ClientWantNewName { new_name: new_name.to_string() }, None) {
                messages.push(format!("name: failed to change your display name: {err}").into());
                *current_name = old_name;
            }
        }
    },
    Command {
        name: "color",
        description: "Change your color",
        signature: "/color <new color>",
        run: |app, new_color| {
            let mut messages = app.shared.messages.lock().unwrap();
            let mut current_color = app.shared.color.lock().unwrap();

            if new_color.is_empty() {
                messages.push("usage: /color <new color>".into());
                return;
            }

            match new_color.parse::<Color>() {
                Ok(new_color_parsed) => {
                    let new_chat_color: ChatColor = new_color_parsed.into();

                    if new_chat_color == *current_color {
                        messages.push("color: you already have the color that you requested".into());
                        return;
                    }

                    let old_color = *current_color;

                    *current_color = new_chat_color;

                    if let Err(err) = send_message(&mut app.stream, Message::ClientWantNewColor { new_color: new_chat_color }, None) {
                        messages.push(format!("color: failed to change your color: {err}").into());
                        *current_color = old_color;
                    }

                    let current_color: Color = current_color.to_owned().into();
                    messages.push(Line::from(vec![
                        Span::from("color: changed your color to "),
                        Span::styled(current_color.to_string(), Style::default().fg(current_color))
                    ]));
                }
                Err(err) => messages.push(format!("color: failed to change your color to '{new_color}': {err}").into())
            }
        }
    },
    Command {
        name: "exit",
        description: "Exits the app",
        signature: "/exit",
        run: exit_app
    },
    Command {
        name: "quit",
        description: "Does the same as /exit",
        signature: "/quit",
        run: exit_app
    }
];

struct App {
    last_tick: Instant,
    stream: TcpStream,
    ui: Ui,
    shared: Arc<Shared>
}

impl App {
    const TICK_RATE: Duration = Duration::from_millis(250);

    fn new(name: &str, stream: TcpStream) -> Self {
        Self {
            last_tick: Instant::now(),
            stream,
            ui: Ui::new(),
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
                        let input = self.ui.input_box.lines().join("\n");
                        let line = input.trim();

                        if line.is_empty() {
                            return Ok(());
                        }

                        if let Some(cmd) = line.strip_prefix("/") {
                            let cmd_name = cmd.split_whitespace().next().unwrap_or("");
                            let args = cmd.strip_prefix(cmd_name).unwrap_or("").trim();

                            if let Some(command) = COMMANDS.iter().find(|c| c.name == cmd_name) {
                                (command.run)(self, args);
                            } else {
                                let mut messages = self.shared.messages.lock().unwrap();
                                messages.push(format!("CMD: Unknown command: {cmd_name}").into());
                            }

                            self.ui.input_box.select_all();
                            self.ui.input_box.cut();

                            return Ok(());
                        }

                        let message = Message::ClientMessage {
                            name: String::new(),
                            color: ChatColor::Reset,
                            msg: line.to_string(),
                        };

                        let timestamp_secs = chrono::Local::now().timestamp();

                        let mut messages = self.shared.messages.lock().unwrap();

                        if let Err(err) = send_message(&mut self.stream, message, Some(timestamp_secs)) {
                            messages.push(format!("ERROR: Failed to send message: {err}").into());
                        } else {
                            let datetime = datetime_from_timestamp(timestamp_secs).to_string();
                            let name = self.shared.name
                                .lock()
                                .unwrap()
                                .to_owned();
                            let color: Color = self.shared.color
                                .lock()
                                .unwrap()
                                .to_owned()
                                .into();
                            messages.push(Line::from(vec![
                                datetime.into(),
                                " ".into(),
                                Span::styled(name, Style::default().fg(color)),
                                format!(": {line}").into()
                            ]));
                        }

                        self.ui.input_box.select_all();
                        self.ui.input_box.cut();
                    }
                    Key::Up => self.ui.vertical_scroll_up(),
                    Key::Down => self.ui.vertical_scroll_down(),
                    _ => {
                        self.ui.input_box.input(input);
                    }
                }
            }
        }

        Ok(())
    }

    fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<(), Box<dyn Error>> {
        spawn_receiver(self.stream.try_clone()?, self.shared.clone());

        while !self.shared.exit.load(Ordering::SeqCst) {
            terminal.draw(|frame| self.ui.draw(frame, &self.shared))?;
            self.handle_events()?;

            if self.last_tick.elapsed() >= Self::TICK_RATE {
                self.last_tick = Instant::now();
            }
        }
        Ok(())
    }
}

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

fn connect(address: &str, name: &str) -> Result<TcpStream, Box<dyn Error>> {
    let sock_addr = address.to_socket_addrs()
        .inspect_err(|err| eprintln!("ERROR: Failed to resolve address {address}: {err}"))?
        .find(|a| a.is_ipv4())
        .unwrap();

    let mut stream = TcpStream::connect_timeout(&sock_addr, Duration::from_secs(20))
        .inspect_err(|err| eprintln!("ERROR: Failed to connect: {err}"))?;

    stream.set_read_timeout(Some(Duration::from_secs(20)))
        .inspect_err(|err| eprintln!("ERROR: Failed to set read timeout: {err}"))?;
    stream.set_write_timeout(Some(Duration::from_secs(20)))
        .inspect_err(|err| eprintln!("ERROR: Failed to set write timeout: {err}"))?;

    println!("INFO: Connected to {sock_addr}");
    println!("INFO: Initiating a handshake...");
    init_handshake(&mut stream).inspect_err(|err| eprintln!("ERROR: Handshake failed: {err}"))?;

    send_message(&mut stream, Message::ClientConnected {
        name: name.to_string(),
        color: ChatColor::Reset
    }, None).inspect_err(|err| eprintln!("ERROR: Failed to send your name to the server: {err}"))?;

    Ok(stream)
}

fn prompt(msg: &str) -> String {
    println!("{msg}");
    let mut input = String::new();
    stdin().read_line(&mut input).unwrap();
    input.trim().to_string()
}

fn main() -> Result<(), Box<dyn Error>> {
    println!("INFO: ChaTTY {CHATTY_VERSION}");

    let address = prompt("Enter the server address (ip:port):");
    let name = prompt("Enter your name:");

    if name.is_empty() {
        eprintln!("ERROR: Cant have an empty name mate");
        return Ok(());
    }

    let stream = connect(&address, &name)?;

    let mut terminal = ratatui::init();
    let mut app = App::new(&name, stream);
    let app_result = app.run(&mut terminal);

    ratatui::restore();

    for msg in app.shared.after_disconnect_messages.lock().unwrap().iter() {
        println!("{msg}");
    }

    app_result
}
