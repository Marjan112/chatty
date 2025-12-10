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

struct Client {
    stream: TcpStream,
    name: String,
    buffer: Vec<u8>
}

struct Server {
    listener: TcpListener,
    poll: Poll,
    clients: HashMap<Token, Client>,
    messages: Vec<Message>,
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

    fn init_handshake(&mut self, stream: &mut TcpStream, addr: &SocketAddr) -> bool {
        let disconnect_by_stream = move |stream: &mut TcpStream, addr: &SocketAddr| {
            if let Err(err) = self.poll.registry().deregister(stream) {
                eprintln!("ERROR: Failed to deregister client {addr} from the poll object: {err}");
            }
            let _ = stream.shutdown(Shutdown::Both);
        };

        const MAX_TRIES: i32 = 500;

        let expected_magic = *b"ChaTTY\0\0";

        let mut magic_buf = [0u8; 8];
        // just a hack till i figure this out
        // TODO: implement a proper async handshake
        for i in 1..(MAX_TRIES + 1) {
            match read_magic(stream, &mut magic_buf) {
                Ok(_) => break,
                Err(ref err) if err.kind() == ErrorKind::WouldBlock => {
                    // println!("INFO: Handshake with client {addr} failed, trying again ({i})");

                    if i >= MAX_TRIES {
                        // eprintln!("ERROR: Handshake with client {addr} failed: {err}");
                        disconnect_by_stream(stream, addr);
                        return false;
                    }

                    continue;
                }
                Err(_err) => {
                    // eprintln!("ERROR: Handshake with client {addr} failed: {err}");
                    disconnect_by_stream(stream, addr);
                    return false;
                }
            }
        }

        if magic_buf != expected_magic {
            // println!("INFO: Invalid client {addr} tried to connect");
            disconnect_by_stream(stream, addr);
            return false;
        }

        if let Err(_err) = stream.write_all(&mut magic_buf) {
            // eprintln!("ERROR: Handshake with client {addr} failed: {err}");
            disconnect_by_stream(stream, addr);
            return false;
        }

        if let Err(_err) = stream.peer_addr() {
            // eprintln!("ERROR: Handshake with client {addr} failed: {err}");
            disconnect_by_stream(stream, addr);
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
                            counter += 1;
                            let client_token = Token(counter);
                            match self.poll.registry().register(&mut stream, client_token, Interest::READABLE) {
                                Ok(_) => {
                                    if !self.init_handshake(&mut stream, &addr) {
                                        continue;
                                    }

                                    self.client_incoming(stream, addr, client_token)
                                }
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
                    let _ = send_message_with_timestamp(&mut other_client.stream, &mut broadcast_msg, true);
                }
            }

            self.messages.push(broadcast_msg);
        }
    }

    fn server_broadcast(&mut self, mut msg: Message, reuse_timestamp: bool) {
        for (_, client) in &mut self.clients {
            let _ = send_message_with_timestamp(&mut client.stream, &mut msg, reuse_timestamp);
        }
        self.messages.push(msg);
    }

    fn client_disconnected(&mut self, token: &Token, reason: &str) {
        if let Some(mut client) = self.clients.remove(&token) {
            if client.name.is_empty() {
                match client.stream.peer_addr() {
                    Ok(addr) => println!("INFO: {addr} disconnected prematurely"),
                    Err(err) => eprintln!("ERROR: Failed to get address of the prematurely disconnected client: {err}")
                }
            } else {
                let disconn_msg = Message::ClientDisconnected {
                    timestamp_secs: Local::now().timestamp(),
                    client_name: client.name.clone(),
                    reason: reason.to_string()
                };
                if let Message::ClientDisconnected { timestamp_secs, .. } = disconn_msg {
                    println!("INFO: '{}' disconnected ({} reason: {})",
                        client.name,
                        datetime_from_timestamp(timestamp_secs),
                        reason);
                    self.server_broadcast(disconn_msg, true);
                }
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

            for msg in &mut self.messages {
                let _ = send_message_with_timestamp(&mut client.stream, msg, true);
            }

            self.server_broadcast(Message::ClientConnected {
                timestamp_secs: timestamp_secs,
                client_name: client_name
            }, true);
        }
    }

    fn client_send_list(&mut self, token: &Token) {
        let client_names: Vec<String> =
            self.clients
                .values()
                .map(|other_client| other_client.name.clone())
                .collect();

        if let Some(client) = self.clients.get_mut(token) {
            let _ = send_message(&mut client.stream, Message::ClientList { client_names: client_names });
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
                let maybe_message = match try_receive_message(&mut client.stream, &mut client.buffer) {
                    Ok(message_opt) => message_opt,
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

                if let Some(message) = maybe_message {
                    match message {
                        Message::ClientConnected { timestamp_secs, client_name } => {
                            self.client_connected(&token, timestamp_secs, client_name);
                        }
                        Message::ClientMessage { timestamp_secs, msg, .. } => {
                            self.client_broadcast(&token, timestamp_secs, msg);
                        }
                        Message::GetClientList => {
                            self.client_send_list(&token);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut server = Server::new()?;
    server.listen();
}
