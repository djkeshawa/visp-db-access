//! Credential encryption with cluster UUID as authenticated associated data.
use aes_gcm::{
    aead::{Aead, Payload},
    Aes256Gcm, KeyInit, Nonce,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use rand::RngCore;
use uuid::Uuid;

/// Authenticated credential encryption; debug output never includes the key.
#[derive(Clone)]
pub struct Crypto(Aes256Gcm);
impl std::fmt::Debug for Crypto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Crypto([redacted])")
    }
}
/// Invalid key or ciphertext.
#[derive(Debug, thiserror::Error)]
#[error("invalid encryption key or ciphertext")]
pub struct CryptoError;
impl Crypto {
    /// Initialize a cipher from a 256-bit key.
    pub fn new(key: &[u8; 32]) -> Self {
        Self(Aes256Gcm::new(key.into()))
    }
    /// Decode the required standard-base64 master key.
    pub fn from_base64(key: &str) -> Result<Self, CryptoError> {
        let bytes = STANDARD.decode(key).map_err(|_| CryptoError)?;
        let key: [u8; 32] = bytes.try_into().map_err(|_| CryptoError)?;
        Ok(Self::new(&key))
    }
    /// Encrypt with a fresh 96-bit nonce and the cluster identity.
    pub fn encrypt(&self, cluster: Uuid, secret: &str) -> Result<String, CryptoError> {
        let mut nonce = [0u8; 12];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        let ciphertext = self
            .0
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: secret.as_bytes(),
                    aad: cluster.as_bytes(),
                },
            )
            .map_err(|_| CryptoError)?;
        let mut bytes = nonce.to_vec();
        bytes.extend(ciphertext);
        Ok(format!("v1:{}", STANDARD.encode(bytes)))
    }
    /// Decrypt and authenticate a versioned ciphertext.
    pub fn decrypt(&self, cluster: Uuid, encoded: &str) -> Result<String, CryptoError> {
        let bytes = STANDARD
            .decode(encoded.strip_prefix("v1:").ok_or(CryptoError)?)
            .map_err(|_| CryptoError)?;
        let nonce = bytes.get(..12).ok_or(CryptoError)?;
        let data = bytes.get(12..).ok_or(CryptoError)?;
        let clear = self
            .0
            .decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: data,
                    aad: cluster.as_bytes(),
                },
            )
            .map_err(|_| CryptoError)?;
        String::from_utf8(clear).map_err(|_| CryptoError)
    }
}
