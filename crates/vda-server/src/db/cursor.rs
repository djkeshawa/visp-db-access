//! Opaque keyset pagination cursor; ordered by creation timestamp and UUID.
use crate::error::ApiError;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
/// Last row in a descending page.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Cursor {
    pub created_at: DateTime<Utc>,
    pub id: Uuid,
}
impl Cursor {
    /// Serialize an opaque URL-safe cursor.
    pub fn encode(&self) -> String {
        URL_SAFE_NO_PAD.encode(format!("{}|{}", self.created_at.to_rfc3339(), self.id))
    }
    /// Reject malformed and excessively large cursors.
    pub fn decode(value: &str) -> Result<Self, ApiError> {
        if value.len() > 256 {
            return Err(ApiError::validation("Invalid cursor"));
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(value)
            .map_err(|_| ApiError::validation("Invalid cursor"))?;
        let text =
            std::str::from_utf8(&bytes).map_err(|_| ApiError::validation("Invalid cursor"))?;
        let (date, id) = text
            .split_once('|')
            .ok_or_else(|| ApiError::validation("Invalid cursor"))?;
        Ok(Self {
            created_at: date
                .parse()
                .map_err(|_| ApiError::validation("Invalid cursor"))?,
            id: id
                .parse()
                .map_err(|_| ApiError::validation("Invalid cursor"))?,
        })
    }
}
