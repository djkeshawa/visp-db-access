//! Metadata models and runtime-checked PostgreSQL repositories.
pub mod cursor;
use crate::{error::ApiError, policy::Policy};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stored cluster configuration; credential ciphertext is never serialized.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ClusterRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub engine: String,
    pub provider: String,
    pub region: String,
    pub environment: String,
    pub host: String,
    pub port: i32,
    pub database: String,
    pub username: String,
    #[serde(skip_serializing)]
    pub password_enc: String,
    pub tls_mode: String,
    pub replica_host: Option<String>,
    pub replica_port: Option<i32>,
    pub tags: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
/// Open the metadata pool with UTC JSON timestamps and bounded acquisition.
pub async fn connect(url: &str, max_connections: u32) -> Result<sqlx::PgPool, sqlx::Error> {
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET TIME ZONE 'UTC'")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(url)
        .await
}
/// Apply embedded, versioned schema migrations.
pub async fn migrate(pool: &sqlx::PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}
/// Retrieve a cluster by primary key.
pub async fn cluster(pool: &sqlx::PgPool, id: Uuid) -> Result<ClusterRecord, ApiError> {
    Ok(sqlx::query_as("SELECT * FROM clusters WHERE id=$1")
        .bind(id)
        .fetch_one(pool)
        .await?)
}
/// Load the full cluster policy.
pub async fn policy(pool: &sqlx::PgPool, id: Uuid) -> Result<Policy, ApiError> {
    let value: serde_json::Value =
        sqlx::query_scalar("SELECT policy FROM cluster_policies WHERE cluster_id=$1")
            .bind(id)
            .fetch_one(pool)
            .await?;
    serde_json::from_value(value).map_err(|_| ApiError::internal())
}
/// Retrieve a public user representation.
pub async fn user(pool: &sqlx::PgPool, id: Uuid) -> Result<crate::auth::User, ApiError> {
    Ok(sqlx::query_as(
        "SELECT id,email,name,org_role,disabled,created_at,last_login_at FROM users WHERE id=$1",
    )
    .bind(id)
    .fetch_one(pool)
    .await?)
}
/// Parse a UUID supplied by the caller.
pub fn id(value: &str) -> Result<Uuid, ApiError> {
    value
        .parse()
        .map_err(|_| ApiError::validation("Invalid UUID"))
}
/// Bounded keyset page parameters.
#[derive(Debug, Deserialize, Default)]
pub struct Page {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
    pub cluster_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub status: Option<String>,
    pub action: Option<String>,
    pub actor_id: Option<Uuid>,
}
impl Page {
    /// Validated page size and optional cursor.
    pub fn bounds(&self) -> Result<(i64, Option<cursor::Cursor>), ApiError> {
        let limit = self.limit.unwrap_or(50);
        if !(1..=200).contains(&limit) {
            return Err(ApiError::validation("Limit must be 1..=200"));
        }
        Ok((
            limit,
            self.cursor
                .as_deref()
                .map(cursor::Cursor::decode)
                .transpose()?,
        ))
    }
}
/// Build a contract page after fetching limit + 1 rows.
pub fn page(mut rows: Vec<serde_json::Value>, limit: i64) -> Result<serde_json::Value, ApiError> {
    let more = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    let next = if more {
        rows.last()
            .map(|row| -> Result<String, ApiError> {
                let created_at = serde_json::from_value(row["created_at"].clone())
                    .map_err(|_| ApiError::internal())?;
                let id =
                    serde_json::from_value(row["id"].clone()).map_err(|_| ApiError::internal())?;
                Ok(cursor::Cursor { created_at, id }.encode())
            })
            .transpose()?
    } else {
        None
    };
    Ok(serde_json::json!({"items":rows,"next_cursor":next}))
}
