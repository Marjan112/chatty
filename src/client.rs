use mio::{
    net::TcpStream,
    Token, Poll, Events, Interest
};
use std::{
    net::Shutdown,
    io::{self, Write, Read},
    thread,
    sync::{Arc, Mutex},
    error::Error
};

const RECEIVER_TOKEN: Token = Token(0);

fn main() -> Result<(), Box<dyn Error>> {
    let mut input_server_address = String::new();
    print!("Enter the server address: ");
    let _ = io::stdout().flush();
    io::stdin().read_line(&mut input_server_address).unwrap();
    let server_address = input_server_address.trim().parse().map_err(|error| {
        eprintln!("Failed to parse the address: {error}");
        error
    })?;

    println!("Connecting...");
    let client = Arc::new(Mutex::new(TcpStream::connect(server_address).map_err(|error| {
        eprintln!("Failed to connect: {error}");
        error
    })?));
    println!("Connected");

    print!("Enter your name (max 20 chars): ");
    let _ = io::stdout().flush();
    let mut input_name = String::new();
    io::stdin().read_line(&mut input_name).unwrap();
    let name = input_name.trim();

    if name.len() > 20 {
        println!("Name needs to be less than 20 chars");
        return Ok(());
    }
    if name.len() < 4 {
        println!("Name needs to be atleast 4 chars");
        return Ok(());
    }

    {
        if let Ok(mut stream) = client.lock() {
            let _ = stream.write(name.as_bytes());
        }
    }

    println!("Welcome {}! To send a message just type the message and hit enter", name);

    let recv_client = client.clone();

    thread::spawn(move || {
        let mut poll = Poll::new().unwrap();
        let mut events = Events::with_capacity(8);

        {
            let mut stream = recv_client.lock().unwrap();
            poll.registry().register(&mut *stream, RECEIVER_TOKEN, Interest::READABLE).unwrap();
        }

        loop {
            poll.poll(&mut events, None).unwrap();
            for event in &events {
                if event.token() == RECEIVER_TOKEN && event.is_readable() {
                    let mut other_client_msg = [0u8; 128];
                    let mut stream = recv_client.lock().unwrap();
                    if let Ok(received) = stream.read(&mut other_client_msg) {
                        if received > 0 {
                            println!("{}", String::from_utf8_lossy(&other_client_msg));
                        } else if received == 0 {
                            println!("Connection closed by server");
                            let _ = stream.shutdown(Shutdown::Both);
                            return;
                        }
                    }
                }
            }
        }
    });

    let mut input_message = String::new();
    loop {
        let _ = io::stdin().read_line(&mut input_message);
        let message = input_message.trim();

        if let Ok(mut stream) = client.lock() {
            let _ = stream.write(message.as_bytes());
        }
        input_message.clear();
    }
}
