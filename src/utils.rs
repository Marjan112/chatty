#![allow(dead_code)]

use std::{io::{self, Read, Write}, net::TcpStream};
use ratatui::{text::Line, style::Stylize};
use chrono::{format::{DelayedFormat, StrftimeItems}, Local, TimeZone};

use crate::env::CHATTY_VERSION;
use crate::Message;

#[macro_export]
macro_rules! chat_error {
    ($messages:expr, $($arg:tt)*) => {
        $messages.push(Line::styled(format!("ERROR: {}", format!($($arg)*)), Style::default().fg(Color::LightRed)))
    }
}

pub fn init_handshake(stream: &mut TcpStream) -> io::Result<()> {
    stream.write_all(b"ChaTTY\0\0")?;

    let mut server_magic_buf = [0u8; 8];
    stream.read_exact(&mut server_magic_buf)?;

    if server_magic_buf != *b"ChaTTY\0\0" {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Not a ChaTTY server"));
    }

    Ok(())
}

pub fn receive_message(stream: &mut TcpStream) -> io::Result<(i64, Message)> {
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

pub fn datetime_from_timestamp(secs: i64) -> DelayedFormat<StrftimeItems<'static>> {
    Local.timestamp_opt(secs, 0)
        .unwrap()
        .format("%Y-%m-%d %H:%M")
}

pub fn greet_message() -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            "ChaTTY ".yellow(),
            "client ".into(),
            CHATTY_VERSION.yellow()
        ]),
        Line::from(vec![
            "Type and press ".into(),
            "ENTER".yellow(),
            " to send".into()
        ]),
        Line::from(vec![
            "Type ".into(),
            "/help".yellow(),
            " for help".into()
        ]),
        Line::from(vec![
            "Press ".into(),
            "ESC".yellow(),
            " to exit".into()
        ])
    ]
}
