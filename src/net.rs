use std::{
    io,
    net::TcpStream,
    thread,
    sync::{atomic::Ordering, Arc},
    time::Duration
};
use ratatui::{
    text::{Span, Line},
    style::Style
};

use crate::message::*;
use crate::shared::*;

struct Receiver {
    stream: TcpStream,
    buffer: Vec<u8>
}

impl Receiver {
    pub fn new(stream: TcpStream) -> Self {
        stream.set_nonblocking(true).unwrap();
        Self { stream, buffer: Vec::new() }
    }

    pub fn try_recv(&mut self) -> io::Result<Option<(i64, Message)>> {
        receive_message(&mut self.stream, &mut self.buffer)
    }
}

pub fn spawn_receiver(stream: TcpStream, shared: Arc<Shared>) {
    let mut receiver = Receiver::new(stream);

    thread::spawn(move || {
        while !shared.exit.load(Ordering::SeqCst) {
            match receiver.try_recv() {
                Ok(Some((timestamp_secs, message))) => handle_incoming_message(timestamp_secs, message, &shared),
                Ok(None) => thread::sleep(Duration::from_millis(1)),
                Err(ref err)
                    if err.kind() == io::ErrorKind::WouldBlock
                        || err.kind() == io::ErrorKind::TimedOut => continue,
                Err(err) => {
                    let mut after = shared.after_disconnect_messages.lock().unwrap();

                    match err.kind() {
                        io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset => {
                            after.push(String::from("INFO: Server closed the connection"));
                        }
                        _ => after.push(format!("ERROR: {err}"))
                    }

                    shared.exit.store(true, Ordering::SeqCst);
                }
            }
        }
    });
}

fn handle_incoming_message(timestamp_secs: i64, message: Message, shared: &Arc<Shared>) {
    let mut messages = shared.messages.lock().unwrap();
    let datetime = datetime_from_timestamp(timestamp_secs).to_string();

    match message {
        Message::ClientConnected {name, color} => {
            messages.push(Line::from(vec![
                datetime.into(),
                Span::from(" "),
                Span::styled(name, Style::default().fg(color.into())),
                Span::from(" connected"),
            ]));
        }
        Message::ClientDisconnected {name, color, reason} => {
            messages.push(Line::from(vec![
                datetime.into(),
                Span::from(" "),
                Span::styled(name, Style::default().fg(color.into())),
                format!(" disconnected (reason: {reason})").into()
            ]));
        }
        Message::ClientMessage {name, color, msg} => {
            messages.push(Line::from(vec![
                datetime.into(),
                Span::from(" "),
                Span::styled(name, Style::default().fg(color.into())),
                format!(": {msg}").into()
            ]));
        }
        Message::ClientList { clients } => {
            messages.push(Line::from("Connected clients:"));

            for (name, color) in clients {
                messages.push(Line::styled(format!("• {name}"), Style::default().fg(color.into())));
            }
        }
        Message::ClientKicked {name, reason} => {
            if name == *shared.name.lock().unwrap() {
                shared.after_disconnect_messages
                    .lock()
                    .unwrap()
                    .push(format!("INFO: You are kicked from the server (reason: {reason})"));

                        shared.exit.store(true, Ordering::SeqCst);
                    }
                },
        Message::ClientChangedName {old_name, new_name} => messages.push(format!("{datetime} {old_name} changed their name to {new_name}").into()),
        Message::ClientAssignedColor { color } => *shared.color.lock().unwrap() = color,
        Message::NameTaken { old_name } => {
            messages.push("name: new name that you requested is already taken by someone else".into());
            *shared.name.lock().unwrap() = old_name;
        }
        _ => {}
    }
}