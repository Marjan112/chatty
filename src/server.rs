use mio::{
    net::{ TcpListener, TcpStream },
    Events, Interest, Poll, Token
};
use std::{
    collections::HashMap,
    io::{ Read, Write, ErrorKind },
    net::SocketAddr,
    error::Error
};

const SERVER_PORT: usize = 6741;

struct Client {
    stream: TcpStream,
    name: String,
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
        println!("[INFO]: Incoming connection from {}", addr); 
        self.clients.insert(token, Client {
            stream: stream,
            name: "".to_string()
        });
    }

    fn client_read(&mut self, token: Token) {
        let mut buf = [0u8; 128];

        let n = match self.clients.get_mut(&token) {
            Some(client) => match client.stream.read(&mut buf) {
                Ok(n) => n,
                Err(err) => {
                    if err.kind() != ErrorKind::WouldBlock {
                        eprintln!("[ERROR]: Failed to read message from '{}': {}", client.name, err);
                        self.clients.remove(&token);
                    }
                    return;
                }
            }
            None => return
        };

        if n == 0 {
            if let Some(client) = self.clients.remove(&token) {
                if client.name.is_empty() {
                    match client.stream.peer_addr() {
                        Ok(addr) => println!("[INFO]: {addr} disconnected"),
                        Err(err) => eprintln!("[ERROR]: Failed to get address of the disconnected client: {err}")
                    }
                } else {
                    println!("[INFO]: '{}' disconnected", client.name)
                }
            }
            return;
        }

        let msg = match str::from_utf8(&buf[..n]) {
            Ok(m) => m,
            Err(_) => return
        };

        if self.clients[&token].name.is_empty() {
            if let Some(first_line) = msg.split('\n').next() {
                if let Some(client) = self.clients.get_mut(&token) {
                    client.name = first_line.to_string();
                    println!("[INFO]: '{}' connected", client.name)
                }
            }
        } else {
            let sender_name = self.clients[&token].name.clone();

            for line in msg.split('\n') {
                if !line.is_empty() {
                    println!("[INFO]: '{}' says: {}", sender_name, line);

                    let broadcast_msg = format!("{}: {}\n", sender_name, line);
                    let recipients: Vec<Token> = self.clients.keys().filter(|&&t| t != token).cloned().collect();

                    for other_token in recipients {
                        if let Some(other_client) = self.clients.get_mut(&other_token) {
                            let _ = other_client.stream.write(broadcast_msg.as_bytes());
                        }
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
