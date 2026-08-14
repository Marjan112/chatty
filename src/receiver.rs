use std::{
    io::{self, Read},
    net::{TcpStream, Shutdown},
    thread,
    sync::{atomic::Ordering, Arc}
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
    let datetime = datetime_from_timestamp(timestamp_secs).to_string();

    match message {
        Message::ClientConnected {name, color} =>
            shared.add_message(Line::from(vec![
                datetime.into(),
                Span::from(" "),
                Span::styled(name, Style::default().fg(color.into())),
                Span::from(" connected"),
            ])),
        Message::ClientDisconnected {name, color, reason} =>
            shared.add_message(Line::from(vec![
                datetime.into(),
                Span::from(" "),
                Span::styled(name, Style::default().fg(color.into())),
                format!(" disconnected (reason: {reason})").into()
            ])),
        Message::ClientMessage {name, color, msg} =>
            shared.add_message(Line::from(vec![
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
        Message::ClientChangedName {old_name, new_name} => shared.add_message(format!("{datetime} {old_name} changed their name to {new_name}").into()),
        Message::ClientAssignedColor { color } => *shared.color.lock().unwrap() = color,
        Message::NameTaken { old_name } => {
            shared.add_message("name: new name that you requested is already taken by someone else".into());
            *shared.name.lock().unwrap() = old_name;
        }
        _ => {}
    }
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

    let decoded = postcard::from_bytes(&buf).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;

    Ok((timestamp, decoded))
}

pub fn spawn_receiver(mut stream: TcpStream, shared: Arc<Shared>)  {
    thread::spawn(move || {
        while shared.connection.load(Ordering::Relaxed) {
            match receive_message(&mut stream) {
                Ok((timestamp_secs, message)) => handle_incoming_message(timestamp_secs, message, &shared),
                Err(ref err) if err.kind() == io::ErrorKind::TimedOut || err.kind() == io::ErrorKind::WouldBlock => continue,
                Err(err) => {
                    match err.kind() {
                        io::ErrorKind::ConnectionReset | io::ErrorKind::UnexpectedEof | io::ErrorKind::BrokenPipe => {
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
