use mio::{
    net::{ TcpListener, TcpStream },
    Events, Interest, Poll, Token
};
use std::{
    collections::HashMap,
    io::{ Read, Write },
    error::Error,
};

const SERVER_PORT: usize = 6741;
const MAX_CLIENTS: usize = 10;

const LISTENER_TOKEN: Token = Token(0);

struct Client {
    stream: TcpStream,
    name: String,
}

struct ServerContext {
    poll: Poll,
    events: Events,
    listener: TcpListener,
    clients: HashMap<Token, Client>
}

impl ServerContext {
    fn create_and_listen() -> Self {
        let mut ctx = ServerContext {
            poll: Poll::new().unwrap(),
            events: Events::with_capacity(128),
            listener: TcpListener::bind(format!("0.0.0.0:{SERVER_PORT}").parse().unwrap()).unwrap(),
            clients: HashMap::new()
        };

        ctx.poll.registry().register(&mut ctx.listener, LISTENER_TOKEN, Interest::READABLE).unwrap();

        println!("Server listening on port {SERVER_PORT}");

        ctx
    }

    fn handle_events(&mut self) -> Result<(), Box<dyn Error>> {
        let mut next_token = Token(LISTENER_TOKEN.0 + 1);

        loop {
            self.poll.poll(&mut self.events, None)?;

            for event in &self.events {
                match event.token() {
                    LISTENER_TOKEN => {
                        loop {
                            match self.listener.accept() {
                                Ok((mut stream, _)) => {
                                    println!("Incoming connection...");

                                    if self.clients.len() > MAX_CLIENTS {
                                        println!("Cant add a new client because the server is full");
                                        continue;
                                    }

                                    let token = next_token;
                                    next_token.0 += 1;

                                    let _ = self.poll.registry().register(&mut stream, token, Interest::READABLE.add(Interest::WRITABLE));
                                    self.clients.insert(token, Client { stream, name: "".to_string() }); 
                                }
                                Err(e) => {
                                    if e.kind() != std::io::ErrorKind::WouldBlock {
                                        eprintln!("Failed to accept: {e}");
                                    }
                                    break;
                                }
                            }
                        }
                    }
                    token => {
                        if let Some(client) = self.clients.get_mut(&token) {
                            let mut buf = [0u8; 128];
                            match client.stream.read(&mut buf) {
                                Ok(n) => {
                                    if n == 0 {
                                        println!("Client '{}' disconnected", client.name);
                                        self.clients.remove(&token);
                                    } else {
                                        let msg = String::from_utf8_lossy(&buf[..n]).to_string();

                                        if client.name.is_empty() {
                                            client.name = msg.trim().to_string();
                                            println!("Client '{}' registered", client.name);
                                        } else {
                                            println!("{} says: {}", client.name, msg);

                                            let broadcast_msg = format!("{}: {}", client.name, msg);

                                            let recipients: Vec<Token> = self.clients.keys().filter(|&&t| t != token).cloned().collect();

                                            for other_token in recipients {
                                                if let Some(other_client) = self.clients.get_mut(&other_token) {
                                                    let _ = other_client.stream.write(broadcast_msg.as_bytes());
                                                }
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    if e.kind() != std::io::ErrorKind::WouldBlock {
                                        eprintln!("Failed to read from the client '{}': {}", self.clients[&token].name, e);
                                        self.clients.remove(&token);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut ctx = ServerContext::create_and_listen();
    ctx.handle_events()?;
    Ok(())
}
