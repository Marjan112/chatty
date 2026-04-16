use std::{
    io::{self, Write, Read},
    net::{TcpStream, Shutdown},
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};
use ratatui::{
    crossterm::event::{self, Event, KeyCode, KeyEventKind},
    style::{Color, Style},
    text::{Line, Span},
    DefaultTerminal,
};

mod chat_color;
use chat_color::*;

mod message;
use message::*;

mod receiver;
use receiver::*;

mod shared;
use shared::*;

mod env;

mod ui;
use ui::*;

mod utils;
use utils::*;

fn exit_app(app: &mut App, _: &str) {
    app.shared.exit.store(true, Ordering::Relaxed);
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
            if let Some(stream) = &mut app.stream {
                if let Err(err) = send_message(stream, Message::GetClientList, None) {
                    messages.push(format!("list: failed to get client list: {err}").into());
                }
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

            if let Some(stream) = &mut app.stream {
                if let Err(err) = send_message(stream, Message::ClientWantNewName { new_name: new_name.to_string() }, None) {
                    messages.push(format!("name: failed to change your display name: {err}").into());
                    *current_name = old_name;
                }
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

                    if let Some(stream) = &mut app.stream {
                        if let Err(err) = send_message(stream, Message::ClientWantNewColor { new_color: new_chat_color }, None) {
                            messages.push(format!("color: failed to change your color: {err}").into());
                            *current_color = old_color;
                        }
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
    },
    Command {
        name: "disconnect",
        description: "Disconnect but does not exit",
        signature: "/disconnect",
        run: |app, _| {
            app.shared.connection.store(false, Ordering::Relaxed);

            app.shared.messages.lock().unwrap().clear();

            if let Some(stream) = app.stream.take() {
                let _ = stream.shutdown(Shutdown::Both);
            }
        }
    }
];

fn init_handshake(stream: &mut TcpStream) -> io::Result<()> {
    stream.write_all(b"ChaTTY\0\0")?;

    let mut server_magic_buf = [0u8; 8];
    stream.read_exact(&mut server_magic_buf)?;

    if server_magic_buf != *b"ChaTTY\0\0" {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Not a ChaTTY server"));
    }

    Ok(())
}

#[derive(Default)]
struct App {
    ui: Ui,
    shared: Arc<Shared>,
    stream: Option<TcpStream>
}

impl App {
    const TICK_RATE: Duration = Duration::from_millis(250);

    fn new() -> Self {
        Default::default()
    }

    fn connect(&mut self, address: String, name: String) {
        let stream_result = TcpStream::connect(address)
            .and_then(|mut stream| {
                stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                stream.set_write_timeout(Some(Duration::from_secs(5)))?;
                init_handshake(&mut stream)?;
                send_message(&mut stream, Message::ClientConnected { name: name.clone(), color: ChatColor::Reset }, None)?;
                spawn_receiver(stream.try_clone()?, self.shared.clone());
                Ok(stream)
            });

        match stream_result {
            Ok(stream) => {
                self.stream = Some(stream);
                self.shared.connection.store(true, Ordering::Relaxed);
                *self.shared.name.lock().unwrap() = name;
                *self.shared.messages.lock().unwrap() = greet_message();
            }
            Err(err) => *self.shared.popup.lock().unwrap() = Some(ActivePopup::Error(format!("Failed to connect: {err}")))
        }
    }

    fn handle_events(&mut self, last_tick: &Instant) -> io::Result<()> {
        let timeout = Self::TICK_RATE.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Esc => {
                            let mut popup = self.shared.popup.lock().unwrap();
                            if popup.is_some() {
                                *popup = None;
                            } else {
                                self.shared.exit.store(true, Ordering::Relaxed);
                            }
                        }
                        KeyCode::Tab if !self.shared.connection.load(Ordering::Relaxed) => self.ui.connect_form.next_field(),
                        KeyCode::Enter => {
                            if !self.shared.connection.load(Ordering::Relaxed) {
                                if self.shared.popup.lock().unwrap().is_none() {
                                    match self.ui.connect_form.focused {
                                        ConnectFormField::Address => self.ui.connect_form.next_field(),
                                        ConnectFormField::Name => {
                                            let address = self.ui.address_input_box.lines()[0].trim().to_string();
                                            let name = self.ui.name_input_box.lines()[0].trim().to_string();

                                            if address.is_empty() {
                                                self.ui.connect_form.focused = ConnectFormField::Address;
                                                return Ok(());
                                            }
                                            if name.is_empty() {
                                                return Ok(());
                                            }

                                            self.connect(address, name);

                                            self.ui.address_input_box.select_all();
                                            self.ui.address_input_box.cut();
                                            self.ui.name_input_box.select_all();
                                            self.ui.name_input_box.cut();

                                            self.ui.connect_form.focused = ConnectFormField::Address;
                                        }
                                    }
                                }
                            } else {
                                let input = self.ui.chat_input_box.lines()[0].trim().to_string();

                                if input.is_empty() {
                                    return Ok(());
                                }

                                if let Some(cmd) = input.strip_prefix("/") {
                                    let cmd_name = cmd.split_whitespace().next().unwrap_or("");
                                    let args = cmd.strip_prefix(cmd_name).unwrap_or("").trim();

                                    if let Some(command) = COMMANDS.iter().find(|c| c.name == cmd_name) {
                                        (command.run)(self, args);
                                    } else {
                                        let mut messages = self.shared.messages.lock().unwrap();
                                        messages.push(format!("CMD: Unknown command: {cmd_name}").into());
                                    }

                                    self.ui.chat_input_box.select_all();
                                    self.ui.chat_input_box.cut();

                                    return Ok(());
                                }

                                let message = Message::ClientMessage {
                                    name: String::new(),
                                    color: ChatColor::Reset,
                                    msg: input.clone(),
                                };

                                let timestamp_secs = chrono::Local::now().timestamp();

                                let mut messages = self.shared.messages.lock().unwrap();

                                match send_message(self.stream.as_mut().unwrap(), message, Some(timestamp_secs)) {
                                    Ok(_) => {
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
                                            format!(": {input}").into()
                                        ]));
                                    }
                                    Err(err) => messages.push(format!("ERROR: Failed to send message: {err}").into())
                                }

                                self.ui.chat_input_box.select_all();
                                self.ui.chat_input_box.cut();
                            }
                        }
                        KeyCode::Up => self.ui.vertical_scroll_up(),
                        KeyCode::Down => self.ui.vertical_scroll_down(),
                        _ => {
                            if !self.shared.connection.load(Ordering::Relaxed) {
                                match self.ui.connect_form.focused {
                                    ConnectFormField::Address => { self.ui.address_input_box.input(key); }
                                    ConnectFormField::Name => { self.ui.name_input_box.input(key); }
                                }
                            } else {
                                self.ui.chat_input_box.input(key);
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        let mut last_tick = Instant::now();

        while !self.shared.exit.load(Ordering::Relaxed) {
            terminal.draw(|frame| {
                if self.shared.connection.load(Ordering::Relaxed) {
                    self.ui.draw_chat(frame, &self.shared);
                } else {
                    self.ui.draw_connect_form(frame);
                    if let Some(popup) = self.shared.popup.lock().unwrap().as_ref() {
                        popup.draw(frame);
                    }
                }
            })?;
            self.handle_events(&last_tick)?;

            if last_tick.elapsed() >= Self::TICK_RATE {
                last_tick = Instant::now();
            }
        }
        Ok(())
    }
}

fn main() -> io::Result<()> {
    let mut terminal = ratatui::init();
    let mut app = App::new();

    let app_result = app.run(&mut terminal);

    ratatui::restore();

    app_result
}
