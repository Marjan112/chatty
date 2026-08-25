use sha2::{Sha256, Digest};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Fingerprint(pub [u8; 32]);

impl Fingerprint {
    pub fn empty() -> Self {
        Default::default()
    }

    pub fn from_certificate(cert: &[u8]) -> Self {
        Self(Sha256::digest(cert).into())
    }
}

impl std::fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let hash = self.0
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        write!(f, "SHA256:{hash}")
    }
}
