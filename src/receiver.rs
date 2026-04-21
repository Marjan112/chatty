use std::{
    io,
    net::{TcpStream, Shutdown},
    thread,
    sync::{atomic::Ordering, Arc},
    time::Duration
};
use ratatui::{
    text::{Span, Line},
    style::{Style, Color}
};

use crate::{
    message::*,
    shared::Shared,
    ui::ActivePopup,
    utils::datetime_from_timestamp,
    chat_error
};

fn handle_incoming_message(timestamp_secs: i64, message: Message, shared: &Shared) {
    let mut messages = shared.messages.lock().unwrap();
    let datetime = datetime_from_timestamp(timestamp_secs).to_string();

    match message {
        Message::ClientConnected {name, color} =>
            messages.push(Line::from(vec![
                datetime.into(),
                Span::from(" "),
                Span::styled(name, Style::default().fg(color.into())),
                Span::from(" connected"),
            ])),
        Message::ClientDisconnected {name, color, reason} =>
            messages.push(Line::from(vec![
                datetime.into(),
                Span::from(" "),
                Span::styled(name, Style::default().fg(color.into())),
                format!(" disconnected (reason: {reason})").into()
            ])),
        Message::ClientMessage {name, color, msg} =>
            messages.push(Line::from(vec![
                datetime.into(),
                Span::from(" "),
                Span::styled(name, Style::default().fg(color.into())),
                format!(": {msg}").into()
            ])),
        Message::ClientList { clients } => *shared.clients.lock().unwrap() = clients,
        Message::ClientKicked {name, reason} => {
            if name == *shared.name.lock().unwrap() {
                *shared.popup.lock().unwrap() = Some(ActivePopup::Info(format!("You have been kicked (reason: {reason})")));
            }
        }
        Message::ClientChangedName {old_name, new_name} => messages.push(format!("{datetime} {old_name} changed their name to {new_name}").into()),
        Message::ClientAssignedColor { color } => *shared.color.lock().unwrap() = color,
        Message::NameTaken { old_name } => {
            messages.push("name: new name that you requested is already taken by someone else".into());
            *shared.name.lock().unwrap() = old_name;
        }
        _ => {}
    }
}

pub fn spawn_receiver(mut stream: TcpStream, shared: Arc<Shared>)  {
    thread::spawn(move || {
        let mut buffer = Vec::new();

        while shared.connection.load(Ordering::Relaxed) {
            match receive_message(&mut stream, &mut buffer) {
                Ok(Some((timestamp_secs, message))) => handle_incoming_message(timestamp_secs, message, &shared),
                Ok(None) => thread::sleep(Duration::from_millis(1)),
                Err(ref err)
                    if err.kind() == io::ErrorKind::WouldBlock
                        || err.kind() == io::ErrorKind::TimedOut => continue,
                Err(err) => {
                    match err.kind() {
                        io::ErrorKind::ConnectionReset | io::ErrorKind::BrokenPipe => {
                            shared.connection.store(false, Ordering::Relaxed);

                            shared.messages.lock().unwrap().clear();
                            shared.clients.lock().unwrap().clear();

                            let _ = stream.shutdown(Shutdown::Both);

                            let mut popup = shared.popup.lock().unwrap();
                            if popup.is_none() {
                                *popup = Some(ActivePopup::Info(String::from("Disconnected from the server")));
                            }
                        }
                        io::ErrorKind::InvalidData => {
                            let mut messages = shared.messages.lock().unwrap();
                            chat_error!(messages, "Received invalid data. A new message kind was probably implemented and your client can't deserialize it. You should try updating your client.");
                        }
                        _ => {
                            shared.connection.store(false, Ordering::Relaxed);

                            shared.messages.lock().unwrap().clear();
                            shared.clients.lock().unwrap().clear();

                            let _ = stream.shutdown(Shutdown::Both);

                            *shared.popup.lock().unwrap() = Some(ActivePopup::Error(err.to_string()));
                        }
                    }
                }
            }
        }
    });
}
