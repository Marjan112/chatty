use std::{
    error::Error,
    io::{self, Read, Write, ErrorKind},
    net::TcpStream,
    thread,
};

fn main() -> Result<(), Box<dyn Error>> {
    println!("Enter the server address (ip:port):");
    let mut input_server_address = String::new();
    io::stdin().read_line(&mut input_server_address).map_err(|err| {
        eprintln!("[ERROR]: Failed to read server address: {err}");
        err
    })?;
    let server_address = input_server_address.trim();

    let mut stream = TcpStream::connect(server_address).and_then(|stream| {
        stream.set_nonblocking(true)?;
        Ok(stream)
    }).map_err(|err| {
        eprintln!("[ERROR]: Failed to connect: {err}");
        err
    })?;

    println!("Connected");
    println!("Enter your name (4-20):");
    let mut input_name = String::new();
    io::stdin().read_line(&mut input_name).map_err(|err| {
        eprintln!("[ERROR]: Failed to read name: {err}");
        err
    })?;
    let name = input_name.trim();
    if name.len() < 4 || name.len() > 20 {
        eprintln!("[ERROR]: Invalid name length");
        return Ok(());
    }

    let _ = stream.write(input_name.as_bytes());
    println!("[INFO]: Welcome {}! To send a message just type the message and hit enter", name);

    let mut stream_clone = stream.try_clone()?;
    thread::spawn(move || {
        let mut buf = [0u8; 128];
        loop {
            match stream_clone.read(&mut buf) {
                Ok(n) => {
                    if n > 0 {
                        if let Ok(msg) = str::from_utf8(&buf[..n]) {
                            for line in msg.split('\n') {
                                if !line.is_empty() {
                                    println!("{line}");
                                }
                            }
                        }
                    } else if n == 0 {
                        println!("[INFO]: Server closed connection");
                        break;
                    }
                }
                Err(e) => {
                    if e.kind() != ErrorKind::WouldBlock {
                        eprintln!("[ERROR]: {e}");
                        break;
                    }
                }
            }
        }
    });

    loop {
        let mut input_msg = String::new();
        io::stdin().read_line(&mut input_msg).map_err(|err| {
            eprintln!("[ERROR]: Failed to read message: {err}");
            err
        })?;
        if input_msg.is_empty() {
            continue;
        }

        if let Err(err) = stream.write_all(input_msg.as_bytes()) {
            eprintln!("[ERROR]: Failed to send: {err}");
            break;
        }
    }

    Ok(())
}
