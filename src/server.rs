use mio::{
    net::{ TcpListener, TcpStream },
    Events, Interest, Poll, Token
};
use std::{
    collections::HashMap,
    error::Error,
    io::{self, ErrorKind, Read, Write},
    net::{Shutdown, SocketAddr},
};
use chrono::Local;

mod message;
use message::*;

mod env;
use env::*;

struct Client {
    stream: TcpStream,
    name: String,
    buffer: Vec<u8>
}

struct Server {
    listener: TcpListener,
    poll: Poll,
    clients: HashMap<Token, Client>,
    messages: Vec<(i64, Message)>,
}

fn read_magic(stream: &mut TcpStream, magic: &mut [u8; 8]) -> io::Result<()> {
    let mut read = 0;
    while read < 8 {
        match stream.read(&mut magic[read..]) {
            Ok(0) => return Err(ErrorKind::ConnectionReset.into()),
            Ok(n) => read += n,
            Err(ref err)
                if err.kind() == ErrorKind::WouldBlock => return Err(ErrorKind::WouldBlock.into()),
            Err(err) => return Err(err),
        }
    }

    Ok(())
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
            listener: listener,
            poll: poll,
            clients: HashMap::new(),
            messages: Vec::new(),
        })
    }

    fn init_handshake(&mut self, stream: &mut TcpStream) -> bool {
        const MAX_TRIES: i32 = 500;

        let expected_magic = *b"ChaTTY\0\0";

        let mut magic_buf = [0u8; 8];
        // just a hack till i figure this out
        // TODO: implement a proper async handshake
        for i in 1..(MAX_TRIES + 1) {
            match read_magic(stream, &mut magic_buf) {
                Ok(_) => break,
                Err(ref err) if err.kind() == ErrorKind::WouldBlock => {
                    if i >= MAX_TRIES {
                        let _ = stream.shutdown(Shutdown::Both);
                        return false;
                    }

                    continue;
                }
                Err(_err) => {
                    let _ = stream.shutdown(Shutdown::Both);
                    return false;
                }
            }
        }

        if magic_buf != expected_magic {
            let _ = stream.shutdown(Shutdown::Both);
            return false;
        }

        if stream.write_all(&mut magic_buf).is_err() {
            let _ = stream.shutdown(Shutdown::Both);
            return false;
        }

        if stream.peer_addr().is_err() {
            let _ = stream.shutdown(Shutdown::Both);
            return false;
        }

        return true;
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
            for token in events.iter().map(|ev| ev.token()) {
                match token {
                    Token(0) => match self.listener.accept() {
                        Ok((mut stream, addr)) => {
                            if !self.init_handshake(&mut stream) {
                                continue;
                            }

                            counter += 1;
                            let client_token = Token(counter);

                            match self.poll.registry().register(&mut stream, client_token, Interest::READABLE) {
                                Ok(_) => self.client_incoming(stream, addr, client_token),
                                Err(err) => eprintln!("ERROR: Failed to register client in the poll object: {err}")
                            }
                        }
                        Err(err) if err.kind() != ErrorKind::WouldBlock => {
                            eprintln!("ERROR: Failed to accept client: {err}");
                        }
                        Err(_) => {}
                    },
                    token => self.client_read(token)
                }
            }
        }
    }

    fn client_incoming(&mut self, stream: TcpStream, addr: SocketAddr, token: Token) {
        println!("INFO: Incoming connection from {addr}");
        self.clients.insert(token, Client {
            stream: stream,
            name: String::new(),
            buffer: Vec::new()
        });
    }

    fn client_broadcast(&mut self, sender_token: &Token, timestamp_secs: i64, msg: String) {
        if let Some(client) = self.clients.get(sender_token) {
            let client_name = client.name.clone();
            println!("INFO: ({}) '{}' says: {}", datetime_from_timestamp(timestamp_secs), client_name, msg);

            let broadcast_msg = Message::ClientMessage {
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
                    let _ = send_message(&mut other_client.stream, broadcast_msg.clone(), Some(timestamp_secs));
                }
            }

            self.messages.push((timestamp_secs, broadcast_msg));
        }
    }

    fn server_broadcast(&mut self, msg: Message, timestamp_secs: i64) {
        for (_, client) in &mut self.clients {
            let _ = send_message(&mut client.stream, msg.clone(), Some(timestamp_secs));
        }
        self.messages.push((timestamp_secs, msg));
    }

    fn client_disconnected(&mut self, token: &Token, reason: &str) {
        if let Some(mut client) = self.clients.remove(&token) {
            if client.name.is_empty() {
                match client.stream.peer_addr() {
                    Ok(addr) => println!("INFO: {addr} disconnected prematurely"),
                    Err(err) => eprintln!("ERROR: Failed to get address of the prematurely disconnected client: {err}")
                }
            } else {
                let timestamp_secs = Local::now().timestamp();

                let disconn_msg = Message::ClientDisconnected {
                    client_name: client.name.clone(),
                    reason: reason.to_string()
                };

                println!("INFO: '{}' disconnected ({} reason: {})",
                    client.name,
                    datetime_from_timestamp(timestamp_secs),
                    reason);

                self.server_broadcast(disconn_msg, timestamp_secs);
            }

            if let Err(err) = self.poll.registry().deregister(&mut client.stream) {
                eprintln!("ERROR: Failed to deregister client '{}' from the poll object: {err}", client.name);
            }
        }
    }

    fn client_connected(&mut self, token: &Token, timestamp_secs: i64, client_name: String) {
        if let Some(client) = self.clients.get_mut(token) {
            client.name = client_name.clone();

            println!("INFO: '{}' connected ({})", client.name, datetime_from_timestamp(timestamp_secs));

            for (timestamp, msg) in &self.messages {
                let _ = send_message(&mut client.stream, msg.clone(), Some(timestamp.clone()));
            }

            self.server_broadcast(
                Message::ClientConnected {
                    client_name: client_name
                },
                timestamp_secs
            );
        }
    }

    fn client_send_list(&mut self, token: &Token) {
        let client_names: Vec<String> =
            self.clients
                .values()
                .map(|other_client| other_client.name.clone())
                .collect();

        if let Some(client) = self.clients.get_mut(token) {
            let _ = send_message(&mut client.stream, Message::ClientList { client_names: client_names }, None);
        }
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
                match try_receive_message(&mut client.stream, &mut client.buffer) {
                    Ok(timestamp_message) => {
                        if let Some((timestamp_secs, message)) = timestamp_message {
                            match message {
                                Message::ClientConnected { client_name } => {
                                    self.client_connected(&token, timestamp_secs, client_name);
                                }
                                Message::ClientMessage { msg, .. } => {
                                    self.client_broadcast(&token, timestamp_secs, msg);
                                }
                                Message::GetClientList => {
                                    self.client_send_list(&token);
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(ref err) if err.kind() == ErrorKind::WouldBlock => break,
                    Err(err) => {
                        let error_message = err.to_string();
                        let mut reason = error_message.as_str();
                        if err.kind() == ErrorKind::ConnectionReset {
                            reason = "connection closed";
                        }
                        self.client_disconnected(&token, reason);
                        break;
                    }
                };
            }
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    println!("INFO: ChaTTY server {CHATTY_VERSION}");
    let mut server = Server::new()?;
    server.listen();
}
