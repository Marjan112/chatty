#![allow(dead_code)]

use std::{io::{self, Write}, fmt};
use chrono::{format::{DelayedFormat, StrftimeItems}, Local, TimeZone};
use bincode::{Decode, Encode};

#[derive(Encode, Decode, Clone)]
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

#[derive(Encode, Decode, Clone)]
pub enum Message {
    ClientConnected {
        client_name: String
    },
    ClientDisconnected {
        client_name: String,
        reason: String
    },
    ClientMessage {
        client_name: String,
        msg: String // `message` is okay but `msg` isnt? this clippy mf be trippin
    },
    GetClientList,
    ClientList {
        client_names: Vec<String>
    },
    ClientKicked {
        client_name: String,
        reason: KickReason
    },
    ClientWantNewName {
        new_name: String
    },
    ClientChangedName {
        old_name: String,
        new_name: String
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

    let encoded = bincode::encode_to_vec(message, bincode::config::standard()).unwrap();
    let encoded_len = encoded.len() as u32;

    stream.write_all(&timestamp_to_send.to_le_bytes())?;
    stream.write_all(&encoded_len.to_le_bytes())?;
    stream.write_all(&encoded)?;

    Ok(())
}
