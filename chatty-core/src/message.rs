use std::{io, fmt};
use serde::{Serialize, Deserialize};
use tokio::io::{AsyncReadExt, AsyncRead, AsyncWriteExt, AsyncWrite};

#[derive(Debug, Serialize, Deserialize, Clone)]
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
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Message {
    ClientConnected {
        name: String,
        color: String
    },
    ClientDisconnected {
        name: String,
        color: String,
        reason: String
    },
    ClientMessage {
        name: String,
        color: String,
        msg: String
    },
    GetClientList,
    ClientList {
        clients: Vec<(String, String)>
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
        new_color: String
    },
    ClientAssignedColor {
        color: String
    },
    NameTaken {
        old_name: String
    }
}

pub async fn receive_message<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<(i64, Message)> {
    let timestamp = reader.read_i64_le().await?;
    let message_len = reader.read_u32_le().await? as usize;
    
    if message_len > 1024 * 1024 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("message is too long: {message_len}")));
    }

    let mut message_bytes = vec![0u8; message_len];
    reader.read_exact(&mut message_bytes).await?;

    let message = postcard::from_bytes(&message_bytes)
        .map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("failed to deserialize message: {err}")
            )
        })?;

    Ok((timestamp, message))
}

pub async fn send_message<W: AsyncWrite + Unpin>(writer: &mut W, message: &Message, timestamp_secs: Option<i64>) -> io::Result<()> {
    let timestamp_to_send = timestamp_secs.unwrap_or(chrono::Local::now().timestamp());

    let encoded = postcard::to_allocvec(message)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let encoded_len = encoded.len() as u32;

    writer.write_i64_le(timestamp_to_send).await?;
    writer.write_u32_le(encoded_len).await?;
    writer.write_all(&encoded).await?;

    Ok(())
}
