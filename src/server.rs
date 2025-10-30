use mio::{
    net::{ TcpListener, TcpStream },
    Events, Interest, Poll, Token
};
use std::{
    collections::HashMap,
    io::ErrorKind,
    net::SocketAddr,
    error::Error
};
use chrono::Local;

mod message;
use message::{Message, try_receive_message, send_message, datetime_from_timestamp};

const SERVER_PORT: usize = 6741;

struct Client {
    stream: TcpStream,
    name: String,
    buffer: Vec<u8>
}

struct Server {
    clients: HashMap<Token, Client>
}

impl Server {
    fn new() -> Self {
        Self {
            clients: HashMap::new()
        }
    }

    fn client_incoming(&mut self, stream: TcpStream, addr: SocketAddr, token: Token) {
        println!("[INFO]: Incoming connection from {addr}");
        self.clients.insert(token, Client {
            stream: stream,
            name: String::new(),
            buffer: Vec::new()
        });
    }

    fn client_broadcast(&mut self, sender_token: &Token, timestamp_secs: i64, client_name: String, msg: String) {
        println!("[INFO]: ({}) '{}' says: {}", datetime_from_timestamp(timestamp_secs), client_name, msg);

        let mut broadcast_msg = Message::ClientMessage {
            timestamp_secs: timestamp_secs,
            client_name: client_name,
            msg: msg
        };

        let recipients: Vec<Token> = self.clients
            .keys()
            .filter(|&&token| token != *sender_token)
            .cloned()
            .collect();

        for other_token in recipients {
            if let Some(other_client) = self.clients.get_mut(&other_token) {
                let _ = send_message(&mut other_client.stream, &mut broadcast_msg, true);
            }
        }
    }

    fn server_broadcast(&mut self, mut msg: Message, reuse_timestamp: bool) {
        for (_, client) in &mut self.clients {
            let _ = send_message(&mut client.stream, &mut msg, reuse_timestamp);
        }
    }

    fn client_disconnected(&mut self, token: &Token, reason: &str) {
        if let Some(client) = self.clients.remove(&token) {
            if client.name.is_empty() {
                match client.stream.peer_addr() {
                    Ok(addr) => println!("[INFO]: {addr} disconnected prematurely"),
                    Err(err) => eprintln!("[ERROR]: Failed to get address of the prematurely disconnected client: {err}")
                }
            } else {
                let disconn_msg = Message::ClientDisconnected {
                    timestamp_secs: Local::now().timestamp(),
                    client_name: client.name.clone(),
                    reason: reason.to_string()
                };
                if let Message::ClientDisconnected { timestamp_secs, .. } = disconn_msg {
                    println!("[INFO]: '{}' disconnected ({} reason: {})",
                        client.name,
                        datetime_from_timestamp(timestamp_secs),
                        reason);
                    self.server_broadcast(disconn_msg, true);
                }
            }
        }
    }

    fn client_connected(&mut self, token: &Token, timestamp_secs: i64, client_name: String) {
        let mut client_name_clone = String::new();
        if let Some(client) = self.clients.get_mut(token) {
            client.name = client_name;
            client_name_clone = client.name.clone();
        }
        println!("[INFO]: '{}' connected ({})", client_name_clone, datetime_from_timestamp(timestamp_secs));
        self.server_broadcast(Message::ClientConnected {
            timestamp_secs: timestamp_secs,
            client_name: client_name_clone
        }, true);
    }

    fn client_read(&mut self, token: Token) {
        let mut read_ops: u32 = 0;
        const READS_PER_TICK: u32 = 32;

        loop {
            read_ops += 1;
            if read_ops > READS_PER_TICK {
                break;
            }
            if let Some(client) = self.clients.get_mut(&token) {
                let maybe_message = match try_receive_message(&mut client.stream, &mut client.buffer) {
                    Ok(message_opt) => message_opt,
                    Err(ref err) if err.kind() == ErrorKind::WouldBlock => break,
                    Err(err) => {
                        self.client_disconnected(&token, &err.to_string());
                        break;
                    }
                };

                if let Some(message) = maybe_message {
                    match message {
                        Message::ClientConnected { timestamp_secs, client_name } => {
                            self.client_connected(&token, timestamp_secs, client_name);
                        }
                        Message::ClientMessage { timestamp_secs, client_name, msg } => {
                            self.client_broadcast(&token, timestamp_secs, client_name, msg);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let address = format!("0.0.0.0:{SERVER_PORT}");
    let mut listener = TcpListener::bind(address.parse().unwrap()).map_err(|err| {
        eprintln!("[ERROR]: Failed to bind {address}: {err}");
        err
    })?;
    let mut poll = Poll::new().map_err(|err| {
        eprintln!("[ERROR]: Failed to create poll object: {err}");
        err
    })?;
    let mut events = Events::with_capacity(1024);
    let mut counter = 0;

    poll.registry().register(&mut listener, Token(counter), Interest::READABLE).map_err(|err| {
        eprintln!("[ERROR]: Failed to register listener in poll object: {err}");
        err
    })?;

    let mut server = Server::new();

    println!("[INFO]: Listening to {address}...");
    loop {
        if let Err(err) = poll.poll(&mut events, None) {
            eprintln!("[ERROR]: Failed to poll: {err}");
            continue;
        }
        for token in events.iter().map(|ev| ev.token()) {
            match token {
                Token(0) => match listener.accept() {
                    Ok((mut stream, addr)) => {
                        counter += 1;
                        let client_token = Token(counter);
                        match poll.registry().register(&mut stream, client_token, Interest::READABLE) {
                            Ok(_) => server.client_incoming(stream, addr, client_token),
                            Err(err) => eprintln!("[ERROR]: Failed to register client in the poll object: {err}")
                        }
                    }
                    Err(err) if err.kind() != ErrorKind::WouldBlock => {
                        eprintln!("[ERROR]: Failed to accept client: {err}");
                    }
                    Err(_) => {}
                },
                token => server.client_read(token)
            }
        }
    }
}
