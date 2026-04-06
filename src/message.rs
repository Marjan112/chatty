#![allow(dead_code)]

use std::{io::{self, Write}, fmt};
use chrono::{format::{DelayedFormat, StrftimeItems}, Local, TimeZone};
use serde::{Serialize, Deserialize};
use ratatui::style::Color;

#[derive(Serialize, Deserialize, Clone)]
pub enum KickReason {
    NameTaken
}

impl fmt::Display for KickReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NameTaken => write!(f, "Name was already taken")
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum ChatColor {
    // The default colors
    Reset,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    LightRed,
    LightGreen,
    LightYellow,
    LightBlue,
    LightMagenta,

    // Any RGB color
    Rgb(u8, u8, u8)
}

impl From<ChatColor> for Color {
    fn from(chat_color: ChatColor) -> Self {
        match chat_color {
            ChatColor::Reset => Color::Reset,
            ChatColor::Red => Color::Red,
            ChatColor::Green => Color::Green,
            ChatColor::Yellow => Color::Yellow,
            ChatColor::Blue => Color::Blue,
            ChatColor::Magenta => Color::Magenta,
            ChatColor::Cyan => Color::Cyan,
            ChatColor::LightRed => Color::LightRed,
            ChatColor::LightGreen => Color::LightGreen,
            ChatColor::LightYellow => Color::LightYellow,
            ChatColor::LightBlue => Color::LightBlue,
            ChatColor::LightMagenta => Color::LightMagenta,
            ChatColor::Rgb(r, g, b) => Color::Rgb(r, g, b)
        }
    }
}

impl From<Color> for ChatColor {
    fn from(color: Color) -> Self {
        match color {
            Color::Reset => ChatColor::Reset,
            Color::Red => ChatColor::Red,
            Color::Green => ChatColor::Green,
            Color::Yellow => ChatColor::Yellow,
            Color::Blue => ChatColor::Blue,
            Color::Magenta => ChatColor::Magenta,
            Color::Cyan => ChatColor::Cyan,
            Color::LightRed => ChatColor::LightRed,
            Color::LightGreen => ChatColor::LightGreen,
            Color::LightYellow => ChatColor::LightYellow,
            Color::LightBlue => ChatColor::LightBlue,
            Color::LightMagenta => ChatColor::LightMagenta,
            Color::Rgb(r, g, b) => ChatColor::Rgb(r, g, b),
            _ => ChatColor::Reset
        }
    }
}

impl fmt::Display for ChatColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let color: Color = (*self).into();
        write!(f, "{}", color)
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub enum Message {
    ClientConnected {
        name: String,
        color: ChatColor
    },
    ClientDisconnected {
        name: String,
        color: ChatColor,
        reason: String
    },
    ClientMessage {
        name: String,
        color: ChatColor,
        msg: String
    },
    GetClientList,
    ClientList {
        clients: Vec<(String, ChatColor)>
    },
    ClientKicked {
        name: String,
        reason: KickReason
    },
    ClientWantNewName {
        new_name: String
    },
    ClientChangedName {
        old_name: String,
        new_name: String
    },
    ClientWantNewColor {
        new_color: ChatColor
    },
    ClientAssignedColor {
        color: ChatColor
    },
    NameTaken {
        old_name: String
    }
}

// i know this function doesnt have to do anything with the message protocol stuff and that it
// shouldnt be here but since i am using it both in server.rs and client.rs i thought i should put it
// in here temporarily
pub fn datetime_from_timestamp(secs: i64) -> DelayedFormat<StrftimeItems<'static>> {
    Local.timestamp_opt(secs, 0)
        .unwrap()
        .format("%Y-%m-%d %H:%M")
}

pub fn send_message<T: Write>(stream: &mut T, message: Message, timestamp_secs: Option<i64>) -> io::Result<()> {
    let timestamp_to_send;
    if let Some(timestamp) = timestamp_secs {
        timestamp_to_send = timestamp;
    } else {
        timestamp_to_send = chrono::Local::now().timestamp();
    }

    let encoded = postcard::to_allocvec(&message).unwrap();
    let encoded_len = encoded.len() as u32;

    stream.write_all(&timestamp_to_send.to_le_bytes())?;
    stream.write_all(&encoded_len.to_le_bytes())?;
    stream.write_all(&encoded)?;

    Ok(())
}
