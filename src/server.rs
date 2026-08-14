use std::{
    io,
    net::SocketAddr,
    collections::HashMap,
    hash::{Hash, Hasher, DefaultHasher},
    sync::Arc
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, AsyncRead, AsyncWrite},
    net::{TcpListener, TcpStream},
    sync::{RwLock, Mutex, mpsc::{self, Sender, Receiver}}
};
use postcard;
use chrono::Local;
use clap::Parser;

mod chat_color;
use chat_color::*;

mod message;
use message::{Message, KickReason};

mod utils;
use utils::{datetime_from_timestamp, MAX_MESSAGES};

mod env;
use crate::env::CHATTY_VERSION;

struct Client {
    addr: SocketAddr,
    name: String,
    color: ChatColor,
    outgoing_tx: Sender<(Option<i64>, Message)>
}

impl Client {
    fn new(addr: SocketAddr, outgoing_tx: Sender<(Option<i64>, Message)>) -> Self {
        Self {
            addr,
            name: String::new(),
            color: ChatColor::Reset,
            outgoing_tx
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

async fn receive_message<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<(i64, Message)> {
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
    clients: RwLock<HashMap<u64, Client>>,
    messages: Mutex<Vec<(i64, Message)>>
}

impl Server {
    fn new() -> Self {
        Self {
            clients: RwLock::new(HashMap::new()),
            messages: Mutex::new(Vec::new())
        }
    }

    async fn get_messages(&self) -> Vec<(i64, Message)> {
        self.messages.lock().await.clone()
    }

    async fn add_message(&self, timestamp: i64, message: Message) {
        let mut messages = self.messages.lock().await;
        messages.push((timestamp, message));

        let messages_len = messages.len();

        if messages_len > MAX_MESSAGES {
            messages.drain(..messages_len - MAX_MESSAGES);
        }
    }

    async fn client_disconnect<S: std::fmt::Display + AsRef<str>>(&self, client_id: u64, reason: S) {
        let client = {
            let mut clients = self.clients.write().await;
            match clients.remove(&client_id) {
                Some(client) => client,
                None => return
            }
        };

        if client.name.is_empty() {
            println!("INFO: disconnected unauthenticated client {} | {}", client.addr, reason);
            return;
        }

        let timestamp = Local::now().timestamp();

        println!("INFO: `{}` disconnected at {} | {}", client.name, datetime_from_timestamp(timestamp), reason);

        let message = Message::ClientDisconnected {
            name: client.name,
            color: client.color,
            reason: reason.to_string()
        };

        self.broadcast(Some(timestamp), message).await;
    }

    async fn client_send_assigned_color(&self, client_id: u64) -> Option<ChatColor> {
        static DEFAULT_COLORS: &[ChatColor] = &[
            ChatColor::Red,
            ChatColor::Green,
            ChatColor::Yellow,
            ChatColor::Blue,
            ChatColor::Magenta,
            ChatColor::Cyan,
            ChatColor::LightRed,
            ChatColor::LightGreen,
            ChatColor::LightYellow,
            ChatColor::LightBlue,
            ChatColor::LightMagenta
        ];

        let (tx, color) = {
            let mut clients = self.clients.write().await;
            let client = match clients.get_mut(&client_id) {
                Some(client) => client,
                None => return None
            };

            let mut hasher = DefaultHasher::new();
            client.name.hash(&mut hasher);
            let hash = hasher.finish();
            let color_index = hash as usize % DEFAULT_COLORS.len();

            client.color = DEFAULT_COLORS[color_index];

            (client.outgoing_tx.clone(), client.color)
        };

        let message = Message::ClientAssignedColor { color };

        let _ = tx.send((None, message)).await;

        Some(color)
    }

    async fn client_connected(&self, client_id: u64, timestamp: i64, client_name: String) {
        let kick_tx = {
            let mut clients = self.clients.write().await;
            if clients.iter().any(|(_, c)| c.name == client_name) {
                let client = match clients.get(&client_id) {
                    Some(client) => client,
                    None => return
                };
                Some(client.outgoing_tx.clone())
            } else {
                if let Some(client) = clients.get_mut(&client_id) {
                    client.name = client_name.clone();
                }
                None
            }
        };

        if let Some(tx) = kick_tx {
            let message = Message::ClientKicked {
                name: client_name.clone(),
                reason: KickReason::NameTaken
            };

            let _ = tx.send((None, message)).await;

            return;
        }

        println!("INFO: `{}` connected at {}", client_name, datetime_from_timestamp(timestamp));
        
        let color = self.client_send_assigned_color(client_id).await.unwrap_or(ChatColor::Reset);

        let tx = {
            let clients = self.clients.read().await;
        
            match clients.get(&client_id) {
                Some(client) => client.outgoing_tx.clone(),
                None => return
            }
        };

        let messages = self.get_messages().await;

        for (timestamp, message) in messages {
            if tx.send((Some(timestamp), message)).await.is_err() {
                return;
            }
        }

        let message = Message::ClientConnected {
            name: client_name,
            color 
        };
        self.broadcast(Some(timestamp), message).await;
    }

    async fn client_broadcast(&self, client_id: u64, timestamp_secs: i64, msg: String) {
        let (txs, message, client_name) = {
            let clients = self.clients.read().await; 

            let client = match clients.get(&client_id) {
                Some(client) => client,
                None => return
            };

            let txs = clients
                .iter()
                .filter(|(id, _)| **id != client_id)
                .map(|(_, client)| client.outgoing_tx.clone())
                .collect::<Vec<_>>();

            let message = Message::ClientMessage {
                name: client.name.clone(),
                color: client.color,
                msg: msg.clone()
            };

            (txs, message, client.name.clone())
        };

        println!("INFO: ({}) `{}` says: {}", datetime_from_timestamp(timestamp_secs), client_name, msg);

        for tx in txs {
            let _ = tx.send((Some(timestamp_secs), message.clone())).await;
        }

        self.add_message(timestamp_secs, message).await;
    }

    async fn send_client_list(&self, client_id: u64) {
        let (tx, clients) = {
            let clients = self.clients.read().await;
            let client = match clients.get(&client_id) {
                Some(client) => client,
                None => return
            };
            let clients = clients
                .values()
                .filter(|c| !c.name.is_empty())
                .map(|c| (c.name.clone(), c.color))
                .collect();
            (client.outgoing_tx.clone(), clients)
        };

        let _ = tx.send((None, Message::ClientList { clients })).await;
    }

    async fn client_change_name(&self, client_id: u64, new_name: String) {
        let message = {
            let mut clients = self.clients.write().await;
            if clients.iter().any(|(_, other_client)| other_client.name == new_name) {
                let client = match clients.get(&client_id) {
                    Some(client) => client,
                    None => return
                };
                Message::NameTaken { old_name: client.name.clone() }
            } else {
                let client = match clients.get_mut(&client_id) {
                    Some(client) => client,
                    None => return
                };
                let old_name = client.name.clone();

                client.name = new_name.clone();
                
                Message::ClientChangedName {
                    old_name,
                    new_name: new_name
                }
            }
        };

        self.broadcast(None, message).await;
    }

    async fn client_change_color(&self, client_id: u64, new_color: ChatColor) {
        let mut clients = self.clients.write().await;
        if let Some(client) = clients.get_mut(&client_id) {
            client.color = new_color;
        }
    }

    async fn broadcast(&self, timestamp: Option<i64>, msg: Message) {
        let timestamp = timestamp.unwrap_or(Local::now().timestamp());

        let txs: Vec<_> = {
            let clients = self.clients.read().await;
            clients
                .values()
                .map(|c| c.outgoing_tx.clone())
                .collect()
        };

        self.add_message(timestamp, msg.clone()).await;

        for tx in txs {
            let _ = tx.send((Some(timestamp), msg.clone())).await;
        }
    }
}

async fn handle_client(server: Arc<Server>, client_id: u64, mut stream: TcpStream, mut rx: Receiver<(Option<i64>, Message)>) -> io::Result<()> {
    if let Err(err) = init_handshake(&mut stream).await {
        server.client_disconnect(client_id, err.to_string()).await;
        return Ok(());
    }

    let (mut reader, mut writer) = stream.into_split();

    let server_reader = server.clone();
    let reader_task = tokio::spawn(async move {
        loop {
            match receive_message(&mut reader).await {
                Ok((timestamp, message)) => {
                    match message {
                        Message::ClientConnected { name, .. } => server_reader.client_connected(client_id, timestamp, name).await,
                        Message::ClientMessage { msg, .. } => server_reader.client_broadcast(client_id, timestamp, msg).await,
                        Message::GetClientList => server_reader.send_client_list(client_id).await,
                        Message::ClientWantNewName { new_name } => server_reader.client_change_name(client_id, new_name).await,
                        Message::ClientWantNewColor { new_color } => server_reader.client_change_color(client_id, new_color).await,
                        _ => {}
                    };
                }
                Err(err) if matches!(err.kind(), io::ErrorKind::UnexpectedEof | io::ErrorKind::BrokenPipe) => {
                    server_reader.client_disconnect(client_id, "connection closed").await;
                    break;
                }
                Err(err) => {
                    server_reader.client_disconnect(client_id, err.to_string()).await;
                    break;
                }
            }    
        }
    });
    
    let server_writer = server.clone();
    let writer_task = tokio::spawn(async move {
        while let Some((timestamp, message)) = rx.recv().await {
            if let Err(err) = send_message(&mut writer, &message, timestamp).await {
                let reason = match err.kind() {
                    io::ErrorKind::UnexpectedEof | io::ErrorKind::BrokenPipe => String::from("connection closed"),
                    _ => err.to_string()
                };
                server_writer.client_disconnect(client_id, reason).await;
                break;
            }
            if let Message::ClientKicked { reason, .. } = message {
                server_writer.client_disconnect(client_id, format!("kicked: {reason}")).await;
                break;
            }
        }
    });

    let _ = tokio::join!(reader_task, writer_task); 

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

                let (outgoing_tx, outgoing_rx) = mpsc::channel::<(Option<i64>, Message)>(64);

                client_id += 1;
                server.clients.write().await.insert(client_id, Client::new(addr, outgoing_tx));

                let server = server.clone();

                tokio::spawn(async move {
                    handle_client(server, client_id, stream, outgoing_rx).await
                });
            }
            Err(err) => eprintln!("ERROR: failed to accept new client: {err}")
        }
    }
}
