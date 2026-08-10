use std::{
    io,
    net::SocketAddr,
    collections::HashMap,
    sync::Arc
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, AsyncWrite},
    net::{TcpListener, TcpStream},
    sync::{RwLock, mpsc::{self, Sender, Receiver}}
};
use postcard;
use clap::Parser;

mod chat_color;
use chat_color::*;

mod message;
use message::Message;

mod env;
use crate::env::CHATTY_VERSION;

struct Client {
    addr: SocketAddr,
    name: String,
    color: ChatColor,
    tx: Sender<(i64, Message)>
}

impl Client {
    fn new(addr: SocketAddr, tx: Sender<(i64, Message)>) -> Self {
        Self {
            addr,
            name: String::new(),
            color: ChatColor::Reset,
            tx
        }
    }
}

async fn send_message<W: AsyncWrite + Unpin>(stream: &mut W, message: &Message, timestamp_secs: Option<i64>) -> io::Result<()>{
    let timestamp_to_send = timestamp_secs.unwrap_or(chrono::Local::now().timestamp());

    let encoded = postcard::to_allocvec(&message)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let encoded_len = encoded.len() as u32;

    stream.write_i64_le(timestamp_to_send).await?;
    stream.write_u32_le(encoded_len).await?;
    stream.write_all(&encoded).await?;

    Ok(())
}

async fn receive_message(stream: &mut TcpStream) -> io::Result<(i64, Message)> {
    let timestamp = stream.read_i64_le().await?;
    let message_len = stream.read_u32_le().await? as usize;
    
    if message_len > 1024 * 1024 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "message is too long"));
    }

    let mut message_bytes = vec![0u8; message_len];
    stream.read_exact(&mut message_bytes).await?;

    let message = postcard::from_bytes(&message_bytes)
        .map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("failed to deserialize message: {err}")
            )
        })?;

    return Ok((timestamp, message));
}

async fn init_handshake(stream: &mut TcpStream) -> io::Result<()> {
    const EXPECTED_MAGIC: &[u8] = b"ChaTTY\0\0";
    let mut magic = [0u8; 8];

    stream.read_exact(&mut magic).await?;
    
    if magic != EXPECTED_MAGIC {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid client"));
    }

    stream.write_all(EXPECTED_MAGIC).await?;

    Ok(())
}

struct Server {
    clients: RwLock<HashMap<u64, Client>>
}

impl Server {
    fn new() -> Self {
        Self {
            clients: RwLock::new(HashMap::new())
        }
    }

    async fn client_disconnect<S: std::fmt::Display + AsRef<str>>(&self, client_id: u64, reason: S) {
        if let Some(client) = self.clients.read().await.get(&client_id) {
            if client.name.is_empty() {
                println!("INFO: disconnected unauthenticated client {} | {}", client.addr, reason);
            } else {
                println!("INFO: `{}` disconnected | {}", client.name, reason);
            }
        }
    }

    async fn client_connected(&self, client_id: u64, timestamp: i64, name: String) {
        // TODO: kick client that doesnt have unique name 
        // TODO: send the assigned color 
        // TODO: send all of the previous messages
        // TODO: broadcast to all clients
    }

    async fn client_broadcast(&self, client_id: u64, timestamp_secs: i64, msg: String) {
        let (txs, message) = {
            let clients = self.clients.read().await; 

            let client = match clients.get(&client_id) {
                Some(client) => client,
                None => return
            };

            let txs = clients
                .iter()
                .filter(|(id, _)| **id != client_id)
                .map(|(_, client)| client.tx.clone())
                .collect::<Vec<_>>();

            let message = Message::ClientMessage {
                name: client.name.clone(),
                color: client.color,
                msg
            };

            (txs, message)
        };

        for tx in txs {
            let _ = tx.send((timestamp_secs, message.clone())).await;
        }
    }

    async fn send_client_list(&self, client_id: u64) {}

    async fn client_change_name(&self, client_id: u64, new_name: String) {}

    async fn client_change_color(&self, client_id: u64, new_color: ChatColor) {}

    async fn broadcast(&self, timestamp_secs: i64, msg: String) {
        // TODO: broadcast to everyone
    }
}

async fn handle_client(server: Arc<Server>, client_id: u64, mut stream: TcpStream, mut rx: Receiver<(i64, Message)>) -> io::Result<()> {
    init_handshake(&mut stream).await?;
    
    loop {
        tokio::select! {
            result = receive_message(&mut stream) => {
                let (timestamp, message) = result?;
                match message {
                    Message::ClientConnected { name, .. } => server.client_connected(client_id, timestamp, name).await,
                    Message::ClientMessage { msg, .. } => server.client_broadcast(client_id, timestamp, msg).await,
                    Message::GetClientList => server.send_client_list(client_id).await,
                    Message::ClientWantNewName { new_name } => server.client_change_name(client_id, new_name).await,
                    Message::ClientWantNewColor { new_color } => server.client_change_color(client_id, new_color).await,
                    _ => {}
                };
            }
            Some((timestamp, message)) = rx.recv() => {
                send_message(&mut stream, &message, Some(timestamp)).await?;
            }
            else => break
        }
    }

    Ok(())
}

#[derive(Parser)]
#[command(version = CHATTY_VERSION)]
struct Args {
    /// The port that the server will bind to
    #[arg(long)]
    port: Option<u16>
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let args = Args::parse();

    println!("INFO: ChaTTY server {CHATTY_VERSION}");

    let listener = TcpListener::bind(format!("0.0.0.0:{}", args.port.unwrap_or(0))).await?; 
    println!("INFO: listening on port {}...", listener.local_addr()?.port());

    let server = Arc::new(Server::new());
    let mut client_id = 0;

    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                println!("INFO: accepted new client {addr}");

                let (tx, rx) = mpsc::channel::<(i64, Message)>(64);

                client_id += 1;
                server.clients.write().await.insert(client_id, Client::new(addr, tx));

                let server = server.clone();

                tokio::spawn(async move {
                    handle_client(server, client_id, stream, rx).await
                });
            }
            Err(err) => eprintln!("ERROR: failed to accept new client: {err}")
        }
    }
}
