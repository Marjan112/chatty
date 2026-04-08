use std::{io, fmt, error::Error};

#[derive(Debug)]
pub enum HandshakeError {
    IO(io::Error),
    Timeout,
    InvalidMagic
}

impl fmt::Display for HandshakeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HandshakeError::IO(err) => write!(f, "{err}"),
            HandshakeError::Timeout => write!(f, "Timeout expired"),
            HandshakeError::InvalidMagic => write!(f, "Not a ChaTTY server")
        }
    }
}

impl From<io::Error> for HandshakeError {
    fn from(err: io::Error) -> Self {
        HandshakeError::IO(err)
    }
}

impl Error for HandshakeError {}