use std::{
    io::Write,
    fs,
    net::SocketAddr,
    collections::HashMap,
    hash::{Hash, Hasher, DefaultHasher},
    sync::Arc
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, AsyncRead, AsyncWrite},
    net::TcpListener,
    sync::{RwLock, Mutex, mpsc::{self, Sender}}
};
use rustls::{
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
    ServerConfig
};
use rustls_pemfile::{certs, private_key};
use tokio_rustls::TlsAcceptor;
use rcgen::generate_simple_self_signed;
use chrono::Local;
use clap::Parser;

use chatty_core::fingerprint::*;
use chatty_core::message::{Message, KickReason, send_message, receive_message};
use chatty_core::utils::{datetime_from_timestamp, MAX_MESSAGES};
use chatty_core::env::CHATTY_VERSION;

struct Client {
    addr: SocketAddr,
    name: String,
    color: String,
    outgoing_tx: Sender<(Option<i64>, Message)>
}

impl Client {
    fn new(addr: SocketAddr, outgoing_tx: Sender<(Option<i64>, Message)>) -> Self {
        Self {
            addr,
            name: String::new(),
            color: "reset".into(),
            outgoing_tx
        }
    }
}

async fn init_handshake<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S) -> std::io::Result<()> {
    const EXPECTED_MAGIC: &[u8] = b"ChaTTY\0\0";
    let mut magic = [0u8; 8];

    stream.read_exact(&mut magic).await?;
    
    if magic != EXPECTED_MAGIC {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid client"));
    }

    stream.write_all(EXPECTED_MAGIC).await?;

    Ok(())
}

struct ServerIdentity {
    certificate: CertificateDer<'static>,
    private_key: PrivateKeyDer<'static>,
    fingerprint: Fingerprint
}

impl ServerIdentity {
    fn load() -> std::io::Result<Self> {
        let home = std::env::home_dir()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "home directory not found"))?;
        let chatty_config = home.join(".chatty");
        fs::create_dir_all(&chatty_config)?;

        let cert_path = chatty_config.join("server.crt");
        let key_path = chatty_config.join("server.key");
        
        let (certificate, private_key) = if cert_path.exists() && key_path.exists() {
            let cert_file = fs::File::open(&cert_path)?;
            let key_file = fs::File::open(&key_path)?;

            let mut cert_reader = std::io::BufReader::new(cert_file);
            let mut key_reader = std::io::BufReader::new(key_file);

            let mut certs: Vec<CertificateDer> = certs(&mut cert_reader)
                .collect::<Result<_, _>>()
                .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;

            let certificate = certs
                .pop()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "no certificate found"))?;

            let private_key = private_key(&mut key_reader)?
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "no private key found"))?;

            (certificate, private_key)
        } else {
            let cert = generate_simple_self_signed(Vec::new())
                .map_err(|err| std::io::Error::other(err.to_string()))?;

            let certificate = CertificateDer::from(cert.cert.der().to_vec());
            let private_key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der()));

            let mut cert_file = fs::File::create(&cert_path)?;
            let mut key_file = fs::File::create(&key_path)?;

            cert_file.write_all(cert.cert.pem().as_bytes())?;
            key_file.write_all(cert.signing_key.serialize_pem().as_bytes())?;

            (certificate, private_key)
        };

        let fingerprint = Fingerprint::from_certificate(&certificate);

        Ok(Self {
            certificate,
            private_key,
            fingerprint
        })
    }
}

struct Server {
    clients: RwLock<HashMap<u64, Client>>,
    messages: Mutex<Vec<(i64, Message)>>
}

impl Server {
    fn tls_config() -> std::io::Result<TlsAcceptor> {
        let identity = ServerIdentity::load()?; 

        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![identity.certificate], identity.private_key)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("invalid TLS config: {err}")))?;

        println!("INFO: server fingerprint:");
        println!("{}", identity.fingerprint);

        Ok(TlsAcceptor::from(Arc::new(config)))
    }

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

    async fn client_send_assigned_color(&self, client_id: u64) -> Option<String> {
        static DEFAULT_COLORS: &[&str] = &[
            "red",
            "green",
            "yellow",
            "blue",
            "magenta",
            "cyan",
            "lightred",
            "lightgreen",
            "lightyellow",
            "lightblue",
            "lightmagenta"
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

            client.color = DEFAULT_COLORS[color_index].to_string();

            (client.outgoing_tx.clone(), client.color.clone())
        };

        let message = Message::ClientAssignedColor { color: color.clone() };

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
        
        let color = self.client_send_assigned_color(client_id).await.unwrap_or("reset".into());

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
                color: client.color.clone(),
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
                .map(|c| (c.name.clone(), c.color.clone()))
                .collect();
            (client.outgoing_tx.clone(), clients)
        };

        let _ = tx.send((None, Message::ClientList { clients })).await;
    }

    async fn client_change_name(&self, client_id: u64, new_name: String) {
        let does_name_collide = {
            let clients = self.clients.read().await;
            if clients.iter().any(|(_, other_client)| other_client.name == new_name) {
                let client = match clients.get(&client_id) {
                    Some(client) => client,
                    None => return
                };
                Some((client.outgoing_tx.clone(), client.name.clone()))
            } else {
                None
            }
        };

        if let Some((tx, old_name)) = does_name_collide {
            let message = Message::NameTaken { old_name };
            let _ = tx.send((None, message)).await;
            return;
        }

        let message = {
            let mut clients = self.clients.write().await;
            
            let client = match clients.get_mut(&client_id) {
                Some(client) => client,
                None => return
            };
            let old_name = client.name.clone();

            client.name = new_name.clone();
            
            Message::ClientChangedName {
                old_name,
                new_name
            }
        };

        self.broadcast(None, message).await;
    }

    async fn client_change_color(&self, client_id: u64, new_color: String) {
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

async fn handle_client<S>(server: Arc<Server>, client_id: u64, mut stream: S, addr: SocketAddr) -> std::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static
{
    init_handshake(&mut stream).await?;

    let (outgoing_tx, mut outgoing_rx) = mpsc::channel::<(Option<i64>, Message)>(64);

    server.clients
        .write()
        .await
        .insert(client_id, Client::new(addr, outgoing_tx));

    let (mut reader, mut writer) = tokio::io::split(stream);

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
                Err(err) if matches!(err.kind(), std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::BrokenPipe) => {
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
        while let Some((timestamp, message)) = outgoing_rx.recv().await {
            if let Err(err) = send_message(&mut writer, &message, timestamp).await {
                let reason = match err.kind() {
                    std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::BrokenPipe => String::from("connection closed"),
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
    #[arg(long, default_value_t = 0)]
    port: u16
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("failed to install rustls crypto provider");

    let args = Args::parse();

    println!("INFO: ChaTTY server {CHATTY_VERSION}");

    let listener = TcpListener::bind(format!("0.0.0.0:{}", args.port))
        .await
        .inspect_err(|err| eprintln!("ERROR: failed to bind: {err}"))?; 
    let tls_acceptor = Server::tls_config()
        .inspect_err(|err| eprintln!("ERROR: TLS config failed: {err}"))?;

    println!("INFO: listening on port {}...", listener.local_addr()?.port());

    let server = Arc::new(Server::new());
    let mut client_id = 0;

    loop {
        if let Ok((stream, addr)) = listener.accept().await {
            client_id += 1;

            let server = server.clone();
            let tls_acceptor = tls_acceptor.clone();

            tokio::spawn(async move {
                let tls_stream = tls_acceptor.accept(stream).await?;
                handle_client(server, client_id, tls_stream, addr).await
            });
        }
    }
}
