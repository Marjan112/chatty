#![allow(dead_code)]

use std::{io::{self, Write}, fmt};
use serde::{Serialize, Deserialize};

use crate::ChatColor;

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

// Make clippy shut up
#[allow(clippy::enum_variant_names)]
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
