#![allow(dead_code)]

use std::io::{Error, ErrorKind, Read, Write};
use chrono::{format::{DelayedFormat, StrftimeItems}, Local, TimeZone};
use bincode::{Decode, Encode};

#[derive(Encode, Decode)]
pub enum Message {
    ClientConnected {
        timestamp_secs: i64,
        client_name: String
    },
    ClientDisconnected {
        timestamp_secs: i64,
        client_name: String,
        reason: String
    },
    ClientMessage {
        timestamp_secs: i64,
        client_name: String,
        msg: String
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

pub fn send_message<T: Write>(stream: &mut T, message: &mut Message, reuse_timestamp: bool) -> Result<(), Error> {
    if !reuse_timestamp {
        match message {
            Message::ClientConnected { timestamp_secs, .. }
            | Message::ClientDisconnected { timestamp_secs, .. }
            | Message::ClientMessage { timestamp_secs, ..} => *timestamp_secs = chrono::Local::now().timestamp(),
        };
    }

    let encoded = bincode::encode_to_vec(&*message, bincode::config::standard()).unwrap();
    let encoded_len = encoded.len() as u32;

    stream.write_all(&encoded_len.to_le_bytes())?;
    stream.write_all(&encoded)?;

    Ok(())
}

pub fn try_receive_message<T: Read>(stream: &mut T, buffer: &mut Vec<u8>) -> Result<Option<Message>, Error> {
    let mut temp = [0u8; 1024];

    match stream.read(&mut temp) {
        Ok(0) => return Err(Error::new(ErrorKind::UnexpectedEof, "connection closed")),
        Ok(n) => buffer.extend_from_slice(&temp[..n]),
        Err(ref err) if err.kind() == ErrorKind::WouldBlock => return Ok(None),
        Err(err) => return Err(err)
    }

    loop {
        if buffer.len() < 4 {
            return Ok(None);
        }

        let len = u32::from_le_bytes(buffer[0..4].try_into().unwrap()) as usize;
        if buffer.len() < 4 + len {
            return Ok(None);
        }

        let msg_bytes = buffer[4..4 + len].to_vec();
        buffer.drain(0..4 + len);

        let (msg, _): (Message, usize) =
            bincode::decode_from_slice(
                &msg_bytes,
                bincode::config::standard()
            )
            .map_err(|err| Error::new(ErrorKind::InvalidData, err))?;

        return Ok(Some(msg));
    }
}

pub fn receive_message<T: Read>(stream: &mut T) -> Result<Message, Error> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = u32::from_le_bytes(len_buf);

    let mut buf = vec![0u8; len as usize];
    stream.read_exact(&mut buf)?;

    let (decoded, _): (Message, usize) =
        bincode::decode_from_slice(
            &buf,
            bincode::config::standard()
        )
        .unwrap();

    Ok(decoded)
}
