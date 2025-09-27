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
        if let Some(client) = self.clients.get_mut(&token) {
            let mut buf = [0u8; 128];
            match client.stream.read(&mut buf) {
                Ok(0) => {
                    println!("[INFO]: '{}' disconnected", client.name);
                    self.clients.remove(&token);
                    return;
                }
                Ok(n) => {
                    let msg = if let Ok(msg) = str::from_utf8(&buf[..n]) {
                        msg
                    } else {
                        "invalid string"
                    };
                    if client.name.is_empty() {
                        client.name = msg.to_string();
                        println!("[INFO]: '{}' connected", client.name);
                    } else {
                        println!("[INFO]: '{}' says: {}", client.name, msg);
                        for (client_token, client) in self.clients.iter_mut() {
                            if *client_token != token {
                                let _ = client.stream.write(msg.as_bytes()).map_err(|err| {
                                    eprintln!("[ERROR]: Failed to broadcast message from {}: {}", client.name, err);
                                });
                            }
                        }
                    }
                }
                Err(err) => {
                    if err.kind() != ErrorKind::WouldBlock {
                        eprintln!("[ERROR]: Failed to read message from '{}': {}", client.name, err);
                        self.clients.remove(&token);
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
