#![allow(dead_code)]

use std::io::{self, Error, ErrorKind, Read, Write};
use chrono::{format::{DelayedFormat, StrftimeItems}, Local, TimeZone};
use bincode::{Decode, Encode};

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
        msg: String
    },
    GetClientList,
    ClientList {
        client_names: Vec<String>
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

pub fn try_receive_message<T: Read>(stream: &mut T, buffer: &mut Vec<u8>) -> io::Result<Option<(i64, Message)>> {
    let mut temp = [0u8; 1024];

    match stream.read(&mut temp) {
        Ok(0) => return Err(Error::new(ErrorKind::UnexpectedEof, "connection closed")),
        Ok(n) => buffer.extend_from_slice(&temp[..n]),
        Err(ref err) if err.kind() == ErrorKind::WouldBlock => return Ok(None),
        Err(err) => return Err(err)
    }

    loop {
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

        let (msg, _): (Message, usize) =
            bincode::decode_from_slice(
                &msg_bytes,
                bincode::config::standard()
            )
            .map_err(|err| Error::new(ErrorKind::InvalidData, err))?;

        return Ok(Some((timestamp, msg)));
    }
}

pub fn receive_message<T: Read>(stream: &mut T) -> io::Result<(i64, Message)> {
    let mut timestamp_buf = [0u8; 8];
    stream.read_exact(&mut timestamp_buf)?;
    let timestamp = i64::from_le_bytes(timestamp_buf);

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

    Ok((timestamp, decoded))
}
