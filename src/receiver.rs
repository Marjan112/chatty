use std::{
    io,
    net::{TcpStream, Shutdown},
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

fn handle_incoming_message(timestamp_secs: i64, message: Message, shared: &Shared) {
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
        Message::ClientKicked {name, ..} => {
            if name == *shared.name.lock().unwrap() {
                // shared.after_disconnect_messages
                //     .lock()
                //     .unwrap()
                //     .push(format!("INFO: You are kicked from the server (reason: {reason})"));

                // TODO: When the client gets kicked we should notify them with some popup that says
                // the reason for being kicked
            }
        },
        Message::ClientChangedName {old_name, new_name} => messages.push(format!("{datetime} {old_name} changed their name to {new_name}").into()),
        Message::ClientAssignedColor { color } => *shared.color.lock().unwrap() = color,
        Message::NameTaken { old_name } => {
            messages.push("name: new name that you requested is already taken by someone else".into());
            *shared.name.lock().unwrap() = old_name;
        },
        _ => {}
    }
}

pub fn spawn_receiver(mut stream: TcpStream, shared: Arc<Shared>) {
    thread::spawn(move || {
        let mut buffer = Vec::new();

        while !shared.exit.load(Ordering::Relaxed) {
            match receive_message(&mut stream, &mut buffer) {
                Ok(Some((timestamp_secs, message))) => handle_incoming_message(timestamp_secs, message, &shared),
                Ok(None) => thread::sleep(Duration::from_millis(1)),
                Err(ref err)
                    if err.kind() == io::ErrorKind::WouldBlock
                        || err.kind() == io::ErrorKind::TimedOut => continue,
                Err(err) => {
                    // let mut after = shared.after_disconnect_messages.lock().unwrap();

                    match err.kind() {
                        io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset | io::ErrorKind::BrokenPipe => {
                            shared.messages.lock().unwrap().clear();
                            let _ = stream.shutdown(Shutdown::Both);
                            shared.connection.store(false, Ordering::Relaxed);
                            break;
                        }
                        _ => {
                            // TODO: Instead of exiting the application maybe we should just get
                            // back to the connect form and then display the error popup

                            // after.push(format!("ERROR: {err}"));
                            shared.exit.store(true, Ordering::Relaxed);
                        }
                    }
                }
            }
        }
    });
}
