use std::{
    io,
    net::SocketAddr,
    collections::HashMap,
    sync::Arc
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{RwLock, mpsc::{self, Sender, Receiver}}
};
use postcard;
use clap::Parser;

mod chat_color;
use chat_color::*;

mod message;
use message::*;

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

type Clients = Arc<RwLock<HashMap<u64, Client>>>;

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

async fn disconnect<S: std::fmt::Display + AsRef<str>>(client_id: u64, clients: Clients, reason: S) {
    if let Some(client) = clients.read().await.get(&client_id) {
        if client.name.is_empty() {
            println!("INFO: disconnected unauthenticated client {} | {}", client.addr, reason);
        } else {
            println!("INFO: `{}` disconnected | {}", client.name, reason);
        }
    }
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

fn connected(client_id: u64, clients: Clients, timestamp: i64, name: String) {
    // TODO: kick client that doesnt have unique name 
    // TODO: send the assigned color 
    // TODO: send all of the previous messages
    // TODO: broadcast to all clients
}

async fn client_broadcast(client_id: u64, clients: Clients, timestamp_secs: i64, msg: String) {
    // TODO: broadcast to all clients except the sender
    let txs = {
        let clients = clients.read().await;
        clients
            .iter()
            .filter(|(id, _)| **id != client_id)
            .map(|(_, client)| client.tx.clone())
            .collect::<Vec<_>>()
    };

    let message = {
        let clients = clients.read().await; 
        let client = clients.get(&client_id).unwrap();    

        Message::ClientMessage {
            name: client.name.clone(),
            color: client.color,
            msg
        }
    };

    for tx in txs {
        let _ = tx.send((timestamp_secs, message.clone())).await;
    }
}

fn server_broadcast(clients: Clients, timestamp_secs: i64, msg: String) {
    // TODO: broadcast to everyone
}

async fn handle_client(client_id: u64, mut stream: TcpStream, clients: Clients, mut rx: Receiver<(i64, Message)>) -> io::Result<()> {
    init_handshake(&mut stream).await?;
    
    loop {
        tokio::select! {
            result = receive_message(&mut stream) => {}
            Some((timestamp, message)) = rx.recv() => {
                // send_message(&mut stream, message, Some(timestamp));
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

    let clients = Arc::new(RwLock::new(HashMap::<u64, Client>::new()));
    let mut client_id = 0;

    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                println!("INFO: accepted new client {addr}");

                let (tx, rx) = mpsc::channel::<(i64, Message)>(64);

                client_id += 1;
                clients.write().await.insert(client_id, Client::new(addr, tx));

                let clients_clone = clients.clone();

                tokio::spawn(async move {
                    handle_client(client_id, stream, clients_clone, rx).await
                });
            }
            Err(err) => eprintln!("ERROR: failed to accept new client: {err}")
        }
    }
}
