#![allow(dead_code)]

use std::{io::{self, Write, Read}, fmt};
use chrono::{format::{DelayedFormat, StrftimeItems}, Local, TimeZone};
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

pub fn receive_message<T: Read>(stream: &mut T, buffer: &mut Vec<u8>) -> io::Result<Option<(i64, Message)>> {
    loop {
        if buffer.len() >= 12 {
            let len = u32::from_le_bytes(buffer[8..12].try_into().unwrap()) as usize;
            if buffer.len() >= 12 + len {
                break;
            }
        }

        let mut temp = [0u8; 1024];
        match stream.read(&mut temp) {
            Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "connection closed")),
            Ok(n) => buffer.extend_from_slice(&temp[..n]),
            Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => return Ok(None),
            Err(err) => return Err(err)
        }
    }

    if buffer.len() < 8 {
        return Ok(None);
    }
    let timestamp = i64::from_le_bytes(buffer[0..8].try_into().unwrap());

    if buffer.len() < 12 {
        return Ok(None);
    }
    let len = u32::from_le_bytes(buffer[8..12].try_into().unwrap()) as usize;

    if buffer.len() < 12 + len {
        return Ok(None);
    }

    let msg_bytes = buffer[12..(12 + len)].to_vec();

    buffer.drain(0..(12 + len));

    let msg = postcard::from_bytes(&msg_bytes)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, format!("failed to deserialize message: {err}")))?;
        
    Ok(Some((timestamp, msg)))
}