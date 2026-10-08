//! Argon2id password hashing off the asynchronous worker threads.
use crate::error::ApiError;
use argon2::{
    password_hash::{PasswordHash, SaltString},
    Algorithm, Argon2, Params, PasswordHasher, PasswordVerifier, Version,
};
use rand::rngs::OsRng;
/// Enforce bounded password sizes.
pub fn validate_password(password: &str) -> Result<(), ApiError> {
    if !(12..=1024).contains(&password.len()) {
        Err(ApiError::validation("Password must be 12..=1024 bytes"))
    } else {
        Ok(())
    }
}
fn hasher() -> Result<Argon2<'static>, ApiError> {
    let params = Params::new(19456, 2, 1, None).map_err(|_| ApiError::internal())?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}
/// Hash with independent random salt and explicitly chosen Argon2id parameters.
pub async fn hash_password(password: String) -> Result<String, ApiError> {
    validate_password(&password)?;
    tokio::task::spawn_blocking(move || {
        hasher()?
            .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
            .map(|hash| hash.to_string())
            .map_err(|_| ApiError::internal())
    })
    .await
    .map_err(|_| ApiError::internal())?
}
/// Verify using the password-hash implementation's constant-time comparison.
pub async fn verify_password(password: String, encoded: String) -> Result<bool, ApiError> {
    if password.len() > 1024 {
        return Ok(false);
    }
    tokio::task::spawn_blocking(move || {
        let hash = PasswordHash::new(&encoded).map_err(|_| ApiError::internal())?;
        Ok(hasher()?
            .verify_password(password.as_bytes(), &hash)
            .is_ok())
    })
    .await
    .map_err(|_| ApiError::internal())?
}
