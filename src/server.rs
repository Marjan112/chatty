use mio::{
    net::{ TcpListener, TcpStream },
    Events, Interest, Poll, Token
};
use std::{
    collections::HashMap,
    error::Error,
    io::{self, Write, Read},
    net::SocketAddr,
    hash::{Hash, Hasher},
    collections::hash_map::DefaultHasher
};
use chrono::Local;

mod chat_color;
use chat_color::*;

mod message;
use message::*;

mod env;
use env::*;

struct Client {
    stream: TcpStream,
    name: String,
    buffer: Vec<u8>,
    color: ChatColor,
    handshake_finished: bool,
}

impl Client {
    const DEFAULT_COLORS: [ChatColor; 11] = [
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

    fn send_assigned_color(&mut self) {
        let mut hasher = DefaultHasher::new();
        self.name.to_lowercase().hash(&mut hasher);
        let hash = hasher.finish();
        let color_index = (hash as usize) % Client::DEFAULT_COLORS.len();

        self.color = Self::DEFAULT_COLORS[color_index];
        let _ = send_message(&mut self.stream, Message::ClientAssignedColor { color: self.color }, None);
    }

    fn try_recv(&mut self) -> io::Result<Option<(i64, Message)>> {
        receive_message(&mut self.stream, &mut self.buffer)
    }
}

struct Server {
    listener: TcpListener,
    poll: Poll,
    clients: HashMap<Token, Client>,
    messages: Vec<(i64, Message)>,
}

impl Server {
    const LISTENING_ADDRESS: &'static str = "0.0.0.0:6741";

    fn new() -> Result<Self, Box<dyn Error>> {
        let mut listener = TcpListener::bind(Self::LISTENING_ADDRESS.parse().unwrap()).map_err(|err| {
            eprintln!("ERROR: Failed to bind {}: {}", Self::LISTENING_ADDRESS, err);
            err
        })?;
        let poll = Poll::new().map_err(|err| {
            eprintln!("ERROR: Failed to create poll object: {err}");
            err
        })?;

        poll.registry().register(&mut listener, Token(0), Interest::READABLE).map_err(|err| {
            eprintln!("ERROR: Failed to register listener in poll object: {err}");
            err
        })?;

        Ok(Self {
            listener,
            poll,
            clients: HashMap::new(),
            messages: Vec::new(),
        })
    }

    fn listen(&mut self) -> ! {
        let mut events = Events::with_capacity(1024);
        let mut counter = 0;

        println!("INFO: Listening to {}...", Self::LISTENING_ADDRESS);
        loop {
            if let Err(err) = self.poll.poll(&mut events, None) {
                eprintln!("ERROR: Failed to poll: {err}");
                continue;
            }
            for event in events.iter() {
                let token = event.token();
                match token {
                    Token(0) => loop {
                        match self.listener.accept() {
                            Ok((mut stream, addr)) => {
                                counter += 1;
                                let client_token = Token(counter);

                                match self.poll.registry().register(&mut stream, client_token, Interest::READABLE | Interest::WRITABLE) {
                                    Ok(_) => self.client_incoming(stream, addr, client_token),
                                    Err(err) => eprintln!("ERROR: Failed to register client in the poll object: {err}")
                                }
                            }
                            Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                            Err(err) => {
                                eprintln!("ERROR: Failed to accept client: {err}");
                                break;
                            }
                        }
                    },
                    token => {
                        if event.is_readable() {
                            self.client_read(token);
                        }
                    }
                }
            }
        }
    }

    fn client_incoming(&mut self, stream: TcpStream, addr: SocketAddr, token: Token) {
        println!("INFO: Incoming connection from {addr}");
        self.clients.insert(token, Client {
            stream,
            name: String::new(),
            color: ChatColor::Reset,
            buffer: Vec::new(),
            handshake_finished: false
        });
    }

    fn client_broadcast(&mut self, sender_token: &Token, timestamp_secs: i64, msg: String) {
        if let Some(client) = self.clients.get(sender_token) {
            let client_name = client.name.clone();
            if client_name.is_empty() {
                return;
            }

            println!("INFO: ({}) '{}' says: {}", datetime_from_timestamp(timestamp_secs), client_name, msg);

            let broadcast_msg = Message::ClientMessage {name: client_name, color: client.color, msg};

            let recipients: Vec<Token> = self.clients
                .keys()
                .filter(|&&token| token != *sender_token)
                .cloned()
                .collect();

            for other_token in recipients {
                if let Some(other_client) = self.clients.get_mut(&other_token) {
                    let _ = send_message(&mut other_client.stream, broadcast_msg.clone(), Some(timestamp_secs));
                }
            }

            self.messages.push((timestamp_secs, broadcast_msg));
        }
    }

    fn server_broadcast(&mut self, msg: Message, timestamp_secs: i64) {
        for client in self.clients.values_mut() {
            let _ = send_message(&mut client.stream, msg.clone(), Some(timestamp_secs));
        }
        self.messages.push((timestamp_secs, msg));
    }

    fn client_disconnected(&mut self, token: &Token, reason: &str) {
        if let Some(mut client) = self.clients.remove(token) {
            if client.name.is_empty() {
                match client.stream.peer_addr() {
                    Ok(addr) => println!("INFO: {addr} disconnected prematurely (reason: {reason})"),
                    Err(err) => eprintln!("ERROR: Failed to get address of the prematurely disconnected client: {err}")
                }
            } else {
                let timestamp_secs = Local::now().timestamp();

                let disconn_msg = Message::ClientDisconnected {
                    name: client.name.clone(),
                    color: client.color,
                    reason: reason.to_string()
                };

                println!("INFO: '{}' disconnected at {} reason: {}", client.name, datetime_from_timestamp(timestamp_secs), reason);

                self.server_broadcast(disconn_msg, timestamp_secs);
            }

            if let Err(err) = self.poll.registry().deregister(&mut client.stream) {
                eprintln!("ERROR: Failed to deregister client '{}' from the poll object: {}", client.name, err);
            }
        }
    }

    fn kick_client(&mut self, token: &Token, client_name: String, reason: KickReason) {
        if let Some(mut client) = self.clients.remove(token) {
            match client.stream.peer_addr() {
                Ok(addr) => println!("INFO: {addr} was kicked (reason: {reason})"),
                Err(err) => eprintln!("ERROR: Failed to get address of the kicked client: {err}")
            }

            let _ = send_message(&mut client.stream, Message::ClientKicked {name: client_name, reason}, None);

            if let Err(err) = self.poll.registry().deregister(&mut client.stream) {
                eprintln!("ERROR: Failed to deregister client '{}' from the poll object: {}", client.name, err);
            }
        }
    }

    fn client_connected(&mut self, token: &Token, timestamp_secs: i64, client_name: String) {
        if self.clients.iter().any(|(_, c)| { c.name == client_name }) {
            self.kick_client(token, client_name, KickReason::NameTaken);
            return;
        }

        let mut client_color = ChatColor::Reset;

        if let Some(client) = self.clients.get_mut(token) {
            client.name = client_name.clone();

            println!("INFO: '{}' connected at {}", client.name, datetime_from_timestamp(timestamp_secs));

            client.send_assigned_color();
            client_color = client.color;

            for (timestamp, msg) in &self.messages {
                let _ = send_message(&mut client.stream, msg.clone(), Some(*timestamp));
            }
        }

        self.server_broadcast(Message::ClientConnected { name: client_name, color: client_color }, timestamp_secs);
    }

    fn client_send_message(&mut self, token: &Token, message: Message, timestamp_secs: Option<i64>) -> io::Result<()> {
        if let Some(client) = self.clients.get_mut(token) {
            send_message(&mut client.stream, message, timestamp_secs)
        } else {
            Err(io::Error::other("client doesnt exist in the hash map"))
        }
    }

    fn client_send_list(&mut self, token: &Token) {
        let clients: Vec<(String, ChatColor)> =
            self.clients
                .values()
                .filter(|other_client| !other_client.name.is_empty())
                .map(|other_client| (other_client.name.clone(), other_client.color))
                .collect();

        let _ = self.client_send_message(token, Message::ClientList { clients }, None);
    }

    fn client_read(&mut self, token: Token) {
        if self.client_handshake(&token) {
            self.client_read_messages(&token);
        }
    }

    fn client_change_name(&mut self, token: &Token, new_name: String) {
        if let Some(client) = self.clients.get(token) {
            if self.clients.iter().any(|(_, other_client)| other_client.name == new_name) {
                let _ = self.client_send_message(token, Message::NameTaken {old_name: client.name.clone()}, None);
                return;
            }
        }

        let mut old_name = String::new();

        if let Some(client) = self.clients.get_mut(token) {
            old_name = client.name.clone();
            client.name = new_name.clone();
            println!("INFO: '{}' changed their name to '{}'", old_name, client.name);
        }

        let timestamp_secs = Local::now().timestamp();

        self.server_broadcast(Message::ClientChangedName { old_name, new_name }, timestamp_secs);
    }

    fn client_change_color(&mut self, token: &Token, new_color: ChatColor) {
        if let Some(client) = self.clients.get_mut(token) {
            client.color = new_color;
        }
    }

    fn client_read_messages(&mut self, token: &Token) {
        let mut read_ops: u32 = 0;
        const READS_PER_TICK: u32 = 32;

        loop {
            read_ops += 1;
            if read_ops > READS_PER_TICK {
                break;
            }
            if let Some(client) = self.clients.get_mut(token) {
                match client.try_recv() {
                    Ok(timestamp_message) => {
                        if let Some((timestamp_secs, message)) = timestamp_message {
                            match message {
                                Message::ClientConnected { name, .. } => self.client_connected(token, timestamp_secs, name),
                                Message::ClientMessage { msg, .. } => self.client_broadcast(token, timestamp_secs, msg),
                                Message::GetClientList => self.client_send_list(token),
                                Message::ClientWantNewName { new_name } => self.client_change_name(token, new_name),
                                Message::ClientWantNewColor { new_color } => self.client_change_color(token, new_color),
                                _ => {}
                            };
                        }
                    }
                    Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => break,
                    Err(err) => {
                        let error_message = err.to_string();
                        let mut reason = error_message.as_str();
                        match err.kind() {
                            io::ErrorKind::ConnectionReset => reason = "connection closed",
                            io::ErrorKind::InvalidData => reason = "invalid client",
                            _ => {}
                        };
                        self.client_disconnected(token, reason);
                        break;
                    }
                };
            }
        }
    }

    fn client_handshake(&mut self, token: &Token) -> bool {
        let client = match self.clients.get_mut(token) {
            Some(c) => c,
            None => return false,
        };

        if client.handshake_finished {
            return true;
        }

        const EXPECTED_MAGIC: &[u8] = b"ChaTTY\0\0";

        loop {
            if client.buffer.len() >= 8 {
                break;
            }

            let mut temp = [0u8; 8];
            match client.stream.read(&mut temp) {
                Ok(0) => {
                    self.client_disconnected(token, "connection closed");
                    return false;
                }
                Ok(n) => client.buffer.extend_from_slice(&temp[..n]),
                Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => return false,
                Err(err) => {
                    self.client_disconnected(token, &err.to_string());
                    return false;
                }
            }
        }

        if client.buffer[..8] != *EXPECTED_MAGIC {
            self.client_disconnected(token, "invalid client");
            return false;
        }

        client.buffer.drain(..8);

        if let Err(err) = client.stream.write_all(EXPECTED_MAGIC) {
            self.client_disconnected(token, &err.to_string());
            return false;
        }

        client.handshake_finished = true;
        true
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    println!("INFO: ChaTTY server {CHATTY_VERSION}");
    let mut server = Server::new()?;
    server.listen();
}
