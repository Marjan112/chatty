#![allow(clippy::collapsible_if)]

use std::{
    io::{self, Write},
    fs,
    sync::{Arc, atomic::Ordering, Mutex},
    time::{Duration, Instant},
    thread,
    future::Future,
    pin::Pin,
    collections::HashMap
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    crossterm::{
        execute,
        cursor::MoveTo,
        terminal::{
            EnterAlternateScreen, LeaveAlternateScreen,
            disable_raw_mode, enable_raw_mode,
            Clear, ClearType
        },
        event::{
            self,
            Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind,
            EnableMouseCapture, DisableMouseCapture
        }
    },
    style::{Color, Style, Stylize},
    text::{Line, Span},
    DefaultTerminal,
};
use clap::{Parser, builder::NonEmptyStringValueParser};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, AsyncRead, AsyncWrite, WriteHalf},
    net::TcpStream
};
use tokio_rustls::{
    TlsConnector,
    client::TlsStream,
    rustls::{
        ClientConfig,
        client::danger::{ServerCertVerified, ServerCertVerifier},
        pki_types::{CertificateDer, ServerName, UnixTime},
        DigitallySignedStruct, Error as TlsError, SignatureScheme
    }
};

use chatty_core::fingerprint::*;
use chatty_core::message::*;
use chatty_core::env::*;
use chatty_core::utils::*;

mod receiver;
use receiver::*;

mod shared;
use shared::*;

mod ui;
use ui::*;

mod macros;
use macros::chat_error;

fn exit_app(app: &mut App, _: &str) {
    app.shared.exit.store(true, Ordering::Relaxed);
}

fn validate_name(name: &str) -> io::Result<String> {
    let trimmed = name.trim();

    if trimmed.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "name cannot be empty"));
    }
    if trimmed.len() > 25 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "name is too long"));
    }
    if !trimmed.chars().all(|c| c.is_alphanumeric() || c == '.' || c == '_' || c == '-') {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "name must be alphanumeric, can contain dots, underscores and dashes"));
    }

    Ok(trimmed.to_string())
}

fn validate_address(address: &str) -> io::Result<(String, String)> {
    let trimmed = address.trim().to_string();

    if trimmed.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "address cannot be empty"));
    }

    if trimmed.chars().all(|c| c.is_control()) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid address"));
    }

    trimmed
        .rsplit_once(':')
        .map(|(host, port)| (host.to_string(), port.to_string()))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid address"))
}

type CommandFuture<'a> = Pin<Box<dyn Future<Output = ()> + 'a>>;

enum CommandRun {
    Sync(fn(&mut App, &str)),
    Async(for<'a> fn(&'a mut App, &'a str) -> CommandFuture<'a>)
}

struct Command {
    name: &'static str,
    description: &'static str,
    signature: &'static str,
    run: CommandRun
}

const COMMANDS: &[Command] = &[
    Command {
        name: "help",
        description: "Helps, duh",
        signature: "/help",
        run: CommandRun::Sync(|app, _| {
            app.shared.add_message(Line::from("Help:"));

            for Command {description, signature, ..} in COMMANDS {
                app.shared.add_message(format!("• {signature} - {description}").into());
            }
        })
    },
    Command {
        name: "clear",
        description: "Clears the chat",
        signature: "/clear",
        run: CommandRun::Sync(|app, _| app.shared.messages.lock().unwrap().clear())
    },
    Command {
        name: "name",
        description: "Change your display name",
        signature: "/name <new name>",
        run: CommandRun::Async(|app, new_name| Box::pin(async move {
            if new_name.is_empty() {
                app.shared.add_message("usage: /name <new name>".into());
                return;
            }

            match validate_name(new_name) {
                Ok(new_name_validated) => {
                    if new_name_validated == *app.shared.name.lock().unwrap() {
                        app.shared.add_message(format!("name: your display name is already `{new_name}`").into());
                        return;
                    }

                    if let Some(writer) = &mut app.writer {
                        if let Err(err) = send_message(writer, &Message::ClientWantNewName { new_name: new_name_validated.clone() }, None).await {
                            app.shared.add_message(format!("name: failed to change your display name: {err}").into());
                        } else {
                            *app.shared.name.lock().unwrap() = new_name_validated;
                        }
                    }
                }
                Err(err) => app.shared.add_message(format!("name: failed to change your display name: {err}").into())
            }
        }))
    },
    Command {
        name: "color",
        description: "Change your color",
        signature: "/color <new color>",
        run: CommandRun::Async(|app, new_color| Box::pin(async move {
            if new_color.is_empty() {
                app.shared.add_message("usage: /color <new color>".into());
                return;
            }

            match new_color.parse::<Color>() {
                Ok(new_color_parsed) => {
                    if new_color_parsed == *app.shared.color.lock().unwrap() {
                        app.shared.add_message("color: you already have the color that you requested".into());
                        return;
                    }

                    if let Some(writer) = &mut app.writer {
                        match send_message(writer, &Message::ClientWantNewColor { new_color: new_color.to_string() }, None).await {
                            Ok(_) => {
                                let mut current_color = app.shared.color.lock().unwrap(); 
                                *current_color = new_color_parsed;
                                app.shared.add_message(Line::from(vec![
                                    Span::from("color: changed your color to "),
                                    Span::styled(current_color.to_string(), Style::default().fg(*current_color))
                                ]));
                            }
                            Err(err) => app.shared.add_message(format!("color: failed to change your color: {err}").into())
                        }
                    }
                }
                Err(_) => app.shared.add_message(format!("color: `{new_color}` not supported, maybe try hex code for that color?").into())
            }
        }))
    },
    Command {
        name: "exit",
        description: "Exits the app",
        signature: "/exit",
        run: CommandRun::Sync(exit_app)
    },
    Command {
        name: "quit",
        description: "Does the same as /exit",
        signature: "/quit",
        run: CommandRun::Sync(exit_app)
    },
    Command {
        name: "disconnect",
        description: "Disconnect but does not exit",
        signature: "/disconnect",
        run: CommandRun::Sync(|app, _| {
            app.shared.connection.store(false, Ordering::Relaxed);

            app.shared.messages.lock().unwrap().clear();
            app.shared.clients.lock().unwrap().clear();

            if let Some(handle) = app.receiver_task.take() {
                handle.abort();
            }

            app.writer = None;
        })
    }
];

pub fn greet_message() -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            "ChaTTY ".yellow(),
            "client ".into(),
            CHATTY_VERSION.yellow()
        ]),
        Line::from(vec![
            "Type and press ".into(),
            "ENTER".yellow(),
            " to send".into()
        ]),
        Line::from(vec![
            "Type ".into(),
            "/help".yellow(),
            " for help".into()
        ]),
        Line::from(vec![
            "Press ".into(),
            "ESC".yellow(),
            " to exit".into()
        ])
    ]
}

async fn init_handshake<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S) -> io::Result<()> {
    stream.write_all(b"ChaTTY\0\0").await?;

    let mut server_magic_buf = [0u8; 8];
    stream.read_exact(&mut server_magic_buf).await?;

    if server_magic_buf != *b"ChaTTY\0\0" {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Not a ChaTTY server"));
    }

    Ok(())
}

fn set_panic_hook() {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal();
        hook(info);
        std::process::exit(1);
    }));
}

fn init_terminal() -> io::Result<DefaultTerminal> {
    set_panic_hook();
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    execute!(io::stdout(), Clear(ClearType::All))?;
    execute!(io::stdout(), EnableMouseCapture)?;
    let backend = CrosstermBackend::new(io::stdout());
    Terminal::new(backend)
}

fn is_term_linux() -> bool {
    std::env::var("TERM")
        .map(|t| t == "linux")
        .unwrap_or(false)
}

fn restore_terminal() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    execute!(io::stdout(), DisableMouseCapture)?;

    if is_term_linux() {
        execute!(io::stdout(), Clear(ClearType::All))?;
        execute!(io::stdout(), MoveTo(0, 0))?;
    }

    Ok(())
}

fn spawn_event_signaler(tx: std::sync::mpsc::Sender<Event>) {
    thread::spawn(move || {
        loop {
            if let Ok(event) = event::read() && tx.send(event).is_err() {
                break;
            }
        }
    });
}

#[cfg(unix)]
async fn wait_for_close_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut close_signal = signal(SignalKind::hangup()).unwrap();
    close_signal.recv().await;
}

#[cfg(windows)]
async fn wait_for_close_signal() {
    use tokio::signal::windows::ctrl_close;
    let mut close_signal = ctrl_close().unwrap();
    close_signal.recv().await;
}

fn spawn_close_signaler(shared: Arc<Shared>)  {
    tokio::spawn(async move {
        wait_for_close_signal().await;
        shared.exit.store(true, Ordering::Relaxed);
    });
}

#[derive(Debug)]
struct TofuVerifier { 
    received_key_fingerprint: Arc<Mutex<Option<Fingerprint>>>,
    expected_key_fingerprint: Option<Fingerprint>
}

impl TofuVerifier {
    fn new(expected_key_fingerprint: Option<Fingerprint>) -> Self {
        Self {
            received_key_fingerprint: Arc::default(),
            expected_key_fingerprint
        }
    }
}

impl ServerCertVerifier for TofuVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        let received = Fingerprint::from_certificate(end_entity);
        match self.expected_key_fingerprint {
            Some(expected) if received != expected => {
                let error_message = format!(concat!(
                    "REMOTE HOST IDENTIFICATION HAS CHANGED!\n",
                    "Someone could be eavesdropping on you right now (MITM attack)!\n",
                    "It is also possible that the host key has just been changed.\n",
                    "Expected key fingerprint:\n",
                    "{}\n",
                    "Received key fingerprint:\n",
                    "{}"
                ), expected, received);
                Err(TlsError::General(error_message))
            }
            Some(_) | None => {
                *self.received_key_fingerprint.lock().unwrap() = Some(received);
                Ok(ServerCertVerified::assertion())
            }
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, TlsError> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, TlsError> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[derive(Default)]
struct KnownHosts {
    hosts: HashMap<(String, String), Fingerprint>
}

impl KnownHosts {
    fn load() -> io::Result<Self> {
        let home = std::env::home_dir()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "home directory not found"))?;
        let known_hosts_path = home
            .join(".chatty")
            .join("known_hosts");

        if !known_hosts_path.exists() {
            return Ok(Self::default());
        }

        let known_hosts_display = known_hosts_path.display();

        let mut hosts = HashMap::new();

        let contents = fs::read_to_string(&known_hosts_path)?;
        for (i, line) in contents.lines().enumerate() {
            let mut parts = line.split_whitespace();
            let line_number = i + 1;

            let (hostname, port) = parts
                .next()
                .ok_or_else(|| io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{known_hosts_display}:{line_number}: missing hostname:port")
                ))?
                .split_once(':')
                .ok_or_else(|| io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{known_hosts_display}:{line_number}: missing port")
                ))?;

            let fingerprint = parts
                .next()
                .ok_or_else(|| io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{known_hosts_display}:{line_number}: missing fingerprint")
                ))?;

            let fingerprint = fingerprint
                .strip_prefix("SHA256:")
                .ok_or_else(|| io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{known_hosts_display}:{line_number}: fingerprint must begin with `SHA256:` prefix")
                ))?;

            if fingerprint.len() != 64 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{known_hosts_display}:{line_number}: fingerprint must contain 64 hexadecimal characters")
                ));
            }

            let mut fingerprint_result = Fingerprint::empty();

            for j in 0..32 {
                fingerprint_result.0[j] = u8::from_str_radix(&fingerprint[j*2..j*2+2], 16)
                    .map_err(|_| io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("{known_hosts_display}:{line_number}: invalid fingerprint")
                    ))?;
            }

            hosts.insert((hostname.to_string(), port.to_string()), fingerprint_result);
        }

        Ok(Self { hosts })
    }

    fn save(&self) -> io::Result<()> {
        let home = std::env::home_dir()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "home directory not found"))?;
        let chatty_dir_path = home.join(".chatty");
        fs::create_dir_all(&chatty_dir_path)?;
        let known_hosts_path = chatty_dir_path.join("known_hosts");

        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(known_hosts_path)?;

        let known_hosts_from_disk = Self::load()?;

        for ((hostname, port), fingerprint) in &self.hosts {
            if let Some(fingerprint_from_disk) = known_hosts_from_disk.hosts.get(&(hostname.to_string(), port.to_string())) {
                if fingerprint_from_disk != fingerprint {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("two different fingerprints for host `{hostname}:{port}`\nfrom memory: {fingerprint}\nfrom disk: {fingerprint_from_disk}"
                    )));
                }
            } else {
                writeln!(f, "{hostname}:{port} {fingerprint}")?;
            }
        }

        Ok(())
    }

    fn get(&self, host: String, port: String) -> Option<&Fingerprint> {
        self.hosts.get(&(host, port))
    }

    fn add(&mut self, host: String, port: String, fingerprint: Fingerprint) {
        self.hosts.insert((host, port), fingerprint);
    }
}

fn client_config(known_host: Option<Fingerprint>) -> io::Result<(TlsConnector, Arc<TofuVerifier>)> {
    let verifier = Arc::new(TofuVerifier::new(known_host));

    let config = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(verifier.clone())
        .with_no_client_auth();

    Ok((TlsConnector::from(Arc::new(config)), verifier))
}

async fn create_tls_stream(known_hosts: &KnownHosts, host: &str, port: &str) -> io::Result<(TlsStream<TcpStream>, Arc<TofuVerifier>)> {
    let (connector, verifier) = client_config(known_hosts.get(host.to_string(), port.to_string()).cloned())?;

    let raw_tcp = TcpStream::connect(format!("{host}:{port}")).await?;

    let server_name = ServerName::try_from(host.to_string())
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err.to_string()))?;

    let tls_stream = tokio::time::timeout(Duration::from_secs(5), connector.connect(server_name, raw_tcp)).await??;

    Ok((tls_stream, verifier))
}

struct App {
    ui: Ui,
    shared: Arc<Shared>,
    writer: Option<WriteHalf<TlsStream<TcpStream>>>,
    receiver_task: Option<tokio::task::JoinHandle<()>>,
    known_hosts: KnownHosts,
    tls_stream: Option<TlsStream<TcpStream>>
}

impl App {
    fn new() -> io::Result<Self> {
        Ok(Self {
            ui: Ui::default(),
            shared: Arc::default(),
            writer: None,
            receiver_task: None,
            known_hosts: KnownHosts::load()?,
            tls_stream: None
        })
    }

    async fn connect(&mut self, mut tls_stream: TlsStream<TcpStream>) -> io::Result<()> {
        tokio::time::timeout(Duration::from_secs(5), init_handshake(&mut tls_stream)).await??;

        let (reader, mut writer) = tokio::io::split(tls_stream);

        let name = self.shared.name.lock().unwrap().clone();

        send_message(
            &mut writer,
            &Message::ClientConnected {
                name: name.clone(),
                color: "reset".into()
            },
            None
        ).await?;

        self.writer = Some(writer);
        self.shared.connection.store(true, Ordering::Relaxed);
        *self.shared.name.lock().unwrap() = name;
        *self.shared.messages.lock().unwrap() = greet_message();
        self.receiver_task = Some(spawn_receiver(reader, self.shared.clone()));

        Ok(())
    }

    async fn verify_connect_from_cli(&mut self, address: &str, name: &str) -> io::Result<()> {
        let (host, port) = validate_address(address)?;
        let name = validate_name(name)?;

        *self.shared.name.lock().unwrap() = name;

        let (tls_stream, verifier) = create_tls_stream(&self.known_hosts, &host, &port).await?;

        let received_key_fingerprint = verifier.received_key_fingerprint.lock().unwrap().unwrap();

        if verifier.expected_key_fingerprint.is_none() {
            println!("INFO: The authenticity of host `{host}` can't be established.");
            println!("INFO: X.509 key fingerprint is: {received_key_fingerprint}");
            println!("Are you sure you want to continue connecting (yes/no)?");

            loop {
                let mut input = String::new();
                io::stdin().read_line(&mut input)?;

                match input.trim().to_lowercase().as_str() {
                    "yes" => {
                        self.known_hosts.add(host, port, received_key_fingerprint);
                        return self.connect(tls_stream).await;
                    }
                    "no" => return Err(io::Error::other("user doesn't want to continue connecting")),
                    _ => println!("Please enter yes or no.")
                }
            }
        } else {
            self.connect(tls_stream).await
        }
    }

    async fn verify_connect_from_ui(&mut self) -> io::Result<()> {
        let (host, port) = validate_address(&self.ui.address_input_box.lines()[0])?;
        *self.shared.name.lock().unwrap() = validate_name(&self.ui.name_input_box.lines()[0])?;

        let (tls_stream, verifier) = create_tls_stream(&self.known_hosts, &host, &port).await?;

        let fingerprint = verifier.received_key_fingerprint.lock().unwrap().unwrap();

        if verifier.expected_key_fingerprint.is_none() {
            let message = format!(concat!(
                "The authenticity of host `{}` can't be established.\n",
                "X.509 key fingerprint is: {}\n",
                "Are you sure you want to continue connecting?"
            ), host, fingerprint);

            *self.shared.popup.lock().unwrap() = Some(ActivePopup::VerifyConnect {host, port, fingerprint, message} );
            self.tls_stream = Some(tls_stream);
        } else {
            self.connect(tls_stream).await?;
        }

        Ok(())
    }

    async fn handle_key_event(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => self.shared.exit.store(true, Ordering::Relaxed),
            KeyCode::Esc => {
                let mut popup = self.shared.popup.lock().unwrap();
                if let Some(ActivePopup::VerifyConnect {..}) = *popup {
                    return;
                }
                if popup.is_some() {
                    *popup = None;
                } else if self.ui.chat_prompt.completion_state.is_some() {
                    self.ui.chat_prompt.completion_state = None;
                } else {
                    self.shared.exit.store(true, Ordering::Relaxed);
                }
            }
            KeyCode::Tab => {
                if self.shared.connection.load(Ordering::Relaxed) {
                    let selected = if let Some(completion) = &mut self.ui.chat_prompt.completion_state {
                        completion.next().map(str::to_owned)
                    } else {
                        None
                    };

                    if let Some(cmd) = selected {
                        self.ui.chat_prompt.set_text(format!("/{cmd}"));
                    } else {
                        let current_input = self.ui.chat_prompt.textarea.lines()[0].trim();
                        if current_input.starts_with('/') && !current_input.contains(' ') {
                            let input_cmd = &current_input[1..];

                            let matches: Vec<&str> = COMMANDS
                                .iter()
                                .filter(|cmd| cmd.name.starts_with(input_cmd))
                                .map(|cmd| cmd.name)
                                .collect();

                            match matches.len() {
                                0 => {}
                                1 => self.ui.chat_prompt.set_text(format!("/{}", matches[0])),
                                _ => {
                                    let completion_state = CompletionState::new(matches);

                                    if let Some(cmd) = completion_state.selected() {
                                        self.ui.chat_prompt.set_text(format!("/{cmd}"));
                                    }

                                    self.ui.chat_prompt.completion_state = Some(completion_state);
                                }
                            }
                        }
                    }
                } else if self.shared.popup.lock().unwrap().is_none() {
                    self.ui.connect_form.next_field();
                }
            }
            KeyCode::Enter => {
                if !self.shared.connection.load(Ordering::Relaxed) && self.shared.popup.lock().unwrap().is_none() {
                    match self.ui.connect_form.focused {
                        ConnectFormField::Address => {
                            if !self.ui.address_input_box.is_empty() {
                                self.ui.connect_form.next_field();
                            }
                        }
                        ConnectFormField::Name => {
                            if !self.ui.address_input_box.is_empty() && !self.ui.name_input_box.is_empty() {
                                if let Err(err) = self.verify_connect_from_ui().await {
                                    *self.shared.popup.lock().unwrap() = Some(ActivePopup::Error(format!("Failed to connect: {err}")));
                                }
                            }
                        }
                    }
                } else {
                    self.ui.chat_prompt.completion_state = None;

                    let input = self.ui.chat_prompt.textarea.lines()[0].trim().to_string();

                    if input.is_empty() {
                        return;
                    }

                    self.ui.chat_prompt.add_to_history(input.clone());

                    if let Some(cmd) = input.strip_prefix("/") {
                        let cmd_name = cmd.split_whitespace().next().unwrap_or("");
                        let args = cmd.strip_prefix(cmd_name).unwrap_or("").trim();

                        if let Some(command) = COMMANDS.iter().find(|c| c.name == cmd_name) {
                            match command.run {
                                CommandRun::Sync(sync_fn) => sync_fn(self, args),
                                CommandRun::Async(async_fn) => async_fn(self, args).await
                            }
                        } else {
                            self.shared.add_message(format!("CMD: Unknown command: {cmd_name}").into());
                        }

                        self.ui.chat_prompt.textarea.clear();

                        return;
                    }

                    let message = Message::ClientMessage {
                        name: String::new(),
                        color: "reset".into(),
                        msg: input.clone(),
                    };

                    let timestamp_secs = chrono::Local::now().timestamp();

                    match send_message(self.writer.as_mut().unwrap(), &message, Some(timestamp_secs)).await {
                        Ok(_) => {
                            let datetime = datetime_from_timestamp(timestamp_secs).to_string();
                            let name = self.shared.name
                                .lock()
                                .unwrap()
                                .to_owned();
                            let color = self.shared.color
                                .lock()
                                .unwrap();
                            self.shared.add_message(Line::from(vec![
                                datetime.into(),
                                " ".into(),
                                Span::styled(name, Style::default().fg(*color)),
                                format!(": {input}").into()
                            ]));
                        }
                        Err(err) => chat_error!(self.shared, "Failed to send message: {err}")
                    }

                    self.ui.chat_prompt.textarea.clear();
                    self.ui.chat_auto_scroll();
                }
            }
            KeyCode::PageUp => self.ui.chat_page_up(),
            KeyCode::PageDown => self.ui.chat_page_down(),
            KeyCode::Up => self.ui.chat_prompt.history_prev(),
            KeyCode::Down => self.ui.chat_prompt.history_next(),
            other => {
                let popup = self.shared.popup.lock().unwrap().clone();
                if let Some(ActivePopup::VerifyConnect {host, port, fingerprint, ..}) = popup {
                    if let KeyCode::Char('y') = other {
                        let tls_stream = self.tls_stream.take().unwrap();
                        self.known_hosts.add(host, port, fingerprint);
                        if let Err(err) = self.connect(tls_stream).await {
                            *self.shared.popup.lock().unwrap() = Some(ActivePopup::Error(format!("Failed to connect: {err}")));
                        } else {
                            *self.shared.popup.lock().unwrap() = None;
                        }
                    } else if let KeyCode::Char('n') = other {
                        let _ = self.tls_stream.take().unwrap().shutdown().await;
                        *self.shared.popup.lock().unwrap() = None;
                        self.shared.name.lock().unwrap().clear();
                    }
                    return;
                }

                if !self.shared.connection.load(Ordering::Relaxed) {
                    match self.ui.connect_form.focused {
                        ConnectFormField::Address => { self.ui.address_input_box.input(key); }
                        ConnectFormField::Name => { self.ui.name_input_box.input(key); }
                    }
                } else {
                    self.ui.chat_prompt.completion_state = None;
                    self.ui.chat_prompt.textarea.input(key);
                }
            }
        };
    }

    fn handle_mouse_event(&mut self, mouse: MouseEvent) {
        let mouse_pos = (mouse.column, mouse.row).into();
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                if self.ui.chat_window_area.contains(mouse_pos) {
                    self.ui.chat_scroll_up();
                } else if self.ui.client_list_area.contains(mouse_pos) {
                    self.ui.client_list_scroll_up();
                }
            }
            MouseEventKind::ScrollDown => {
                if self.ui.chat_window_area.contains(mouse_pos) {
                    self.ui.chat_scroll_down();
                } else if self.ui.client_list_area.contains(mouse_pos) {
                    self.ui.client_list_scroll_down();
                }
            }
            _ => {}
        }
    }

    async fn handle_events(&mut self, rx: &std::sync::mpsc::Receiver<Event>) {
        while let Ok(event) = rx.try_recv() {
            match event {
                Event::Key(key) if key.is_press() => self.handle_key_event(key).await,
                Event::Mouse(mouse) => self.handle_mouse_event(mouse),
                _ => {}
            }
        }
    }

    async fn run(&mut self) -> io::Result<()> {
        let mut terminal = init_terminal()?;
        let mut get_client_list_timer = Instant::now();
        let (event_tx, event_rx) = std::sync::mpsc::channel();

        // TODO: would it be better to use EventStream?
        spawn_event_signaler(event_tx);
        spawn_close_signaler(self.shared.clone()); 

        while !self.shared.exit.load(Ordering::Relaxed) {
            self.handle_events(&event_rx).await;

            terminal.draw(|frame| {
                if self.shared.connection.load(Ordering::Relaxed) {
                    self.ui.draw_chat(frame, &self.shared);
                } else {
                    if let Some(popup) = self.shared.popup.lock().unwrap().as_ref() {
                        popup.draw(frame);
                    } else {
                        self.ui.draw_connect_form(frame);
                    }
                }
            })?;

            if !self.shared.connection.load(Ordering::Relaxed) {
                self.writer = None;
            }

            if let Some(writer) = &mut self.writer {
                if get_client_list_timer.elapsed() >= Duration::from_secs(1) {
                    let _ = send_message(writer, &Message::GetClientList, None).await;
                    get_client_list_timer = Instant::now();
                }
            }

            // Render at 60 FPS
            tokio::time::sleep(Duration::from_millis(16)).await;
        }

        let _ = restore_terminal();

        self.known_hosts
            .save()
            .inspect_err(|err| eprintln!("ERROR: Failed to save known hosts to database: {err}"))
    }
}

#[derive(Parser)]
#[command(version = CHATTY_VERSION)]
struct Args {
    /// The address for the client to connect to (e.g. localhost:8080)
    #[arg(long, requires = "name", value_parser = NonEmptyStringValueParser::new())]
    address: Option<String>,

    /// Your display name that will be used once you connect 
    #[arg(long, requires = "address", value_parser = NonEmptyStringValueParser::new())]
    name: Option<String>
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let args = Args::parse();
    let mut app = App::new()
        .inspect_err(|err| eprintln!("ERROR: Failed to initialize: {err}"))?;

    if let (Some(address), Some(name)) = (args.address, args.name) {
        println!("INFO: Trying to connect to `{address}` as `{name}`...");
        app.verify_connect_from_cli(&address, &name)
            .await
            .inspect_err(|err| eprintln!("ERROR: Failed to connect: {err}"))?;
    }
    
    app.run().await
}
