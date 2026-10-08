//! Stable JSON errors; infrastructure details remain in structured logs.
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

/// An error that is safe to send to an API caller.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{code}: {message}")]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    pub details: Value,
    pub kind: ErrorKind,
}
/// Typed origin used for cancellation and masked-diagnostic handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Other,
    Database,
    Cancelled,
}
impl ApiError {
    /// Construct a public error.
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            details: Value::Null,
            kind: ErrorKind::Other,
        }
    }
    /// Attach contract-specific structured details.
    pub fn details(mut self, details: impl serde::Serialize) -> Self {
        self.details = serde_json::to_value(details).unwrap_or(Value::Null);
        self
    }
    /// Invalid input.
    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "validation", message)
    }
    /// Missing session or credentials.
    pub fn unauthenticated() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "unauthenticated",
            "Authentication required or invalid credentials",
        )
    }
    /// Insufficient access.
    pub fn forbidden() -> Self {
        Self::new(StatusCode::FORBIDDEN, "forbidden", "Access denied")
    }
    /// Resource does not exist.
    pub fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", "Resource not found")
    }
    /// State transition is not permitted.
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, "conflict", message)
    }
    /// Stored target credentials can't be decrypted with the current master key.
    pub fn credentials_unreadable(cluster: uuid::Uuid) -> Self {
        tracing::error!(
            cluster_id = %cluster,
            "stored credentials cannot be decrypted; VDA_MASTER_KEY differs from the key they were encrypted with"
        );
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "credentials_unreadable",
            "Stored credentials for this cluster can't be decrypted. Restore the previous \
             VDA_MASTER_KEY, or re-enter the cluster password in its settings.",
        )
    }
    /// Internal failure with a generic message.
    pub fn internal() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "Internal server error",
        )
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error":{"code":self.code,"message":self.message,"details":self.details}})),
        )
            .into_response()
    }
}
impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        tracing::error!(error = %error, "metadata operation failed");
        if matches!(error, sqlx::Error::RowNotFound) {
            return Self::not_found();
        }
        if let sqlx::Error::Database(ref db) = error {
            if db.is_unique_violation() {
                return Self::conflict("Resource already exists");
            }
            if db.is_foreign_key_violation() {
                return Self::conflict("Resource is referenced or scope does not exist");
            }
        }
        Self::internal()
    }
}
impl From<vda_connectors::ConnectorError> for ApiError {
    fn from(error: vda_connectors::ConnectorError) -> Self {
        // Connection failures stay generic. Database statement diagnostics are
        // part of the v1 query error contract and are visible to the query owner.
        tracing::warn!(error = %error, "target operation failed");
        let kind = match &error {
            vda_connectors::ConnectorError::Cancelled => ErrorKind::Cancelled,
            vda_connectors::ConnectorError::Database(_) => ErrorKind::Database,
            _ => ErrorKind::Other,
        };
        let message: String = match error {
            vda_connectors::ConnectorError::Database(message) => message,
            vda_connectors::ConnectorError::Cancelled => "Query cancelled".into(),
            vda_connectors::ConnectorError::Timeout => "Query timed out".into(),
            vda_connectors::ConnectorError::TooManyAffectedRows { .. } => {
                "Affected row limit exceeded; transaction rolled back".into()
            }
            _ => "Target database unavailable".into(),
        };
        let mut public = Self::new(StatusCode::BAD_GATEWAY, "upstream", message);
        public.kind = kind;
        public
    }
}

/// JSON extractor with the API error envelope for malformed input.
#[derive(Debug)]
pub struct Input<T>(pub T);
impl<S, T> axum::extract::FromRequest<S> for Input<T>
where
    S: Send + Sync,
    T: serde::de::DeserializeOwned,
{
    type Rejection = ApiError;
    async fn from_request(req: axum::extract::Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) = Json::<Value>::from_request(req, state)
            .await
            .map_err(|_| ApiError::validation("Invalid JSON body"))?;
        crate::sanitation::json(&value)?;
        serde_json::from_value(value)
            .map(Self)
            .map_err(|_| ApiError::validation("Invalid JSON body"))
    }
}
