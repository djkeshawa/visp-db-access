//! Cancellation and connection lifecycle safeguards.
use crate::ConnectorError;
use sqlx::{pool::PoolConnection, Connection, Database};
use std::{
    future::Future,
    ops::{Deref, DerefMut},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

/// An unfinished operation must never return a connection to the pool.
pub(crate) struct GuardedConnection<DB: Database> {
    connection: PoolConnection<DB>,
    clean: bool,
    discard_hook: Option<Box<dyn FnOnce() + Send>>,
}
impl<DB: Database> GuardedConnection<DB> {
    pub(crate) fn new(connection: PoolConnection<DB>) -> Self {
        Self {
            connection,
            clean: false,
            discard_hook: None,
        }
    }
    pub(crate) fn on_discard(&mut self, hook: impl FnOnce() + Send + 'static) {
        self.discard_hook = Some(Box::new(hook));
    }
    pub(crate) fn clear_discard_hook(&mut self) {
        self.discard_hook = None;
    }
    pub(crate) fn clean(&mut self) {
        self.clean = true;
    }
    pub(crate) fn discard(&mut self) {
        self.clean = false;
    }
}
impl<DB: Database> Deref for GuardedConnection<DB> {
    type Target = DB::Connection;
    fn deref(&self) -> &Self::Target {
        &self.connection
    }
}
impl<DB: Database> DerefMut for GuardedConnection<DB> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.connection
    }
}
impl<DB: Database> Drop for GuardedConnection<DB> {
    fn drop(&mut self) {
        if !self.clean {
            self.connection.close_on_drop();
            if let Some(hook) = self.discard_hook.take() {
                hook();
            }
        }
    }
}

pub(crate) async fn race<T>(
    future: impl Future<Output = Result<T, ConnectorError>>,
    cancel: &CancellationToken,
    timeout: Duration,
) -> Result<T, ConnectorError> {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(ConnectorError::Cancelled),
        _ = tokio::time::sleep(timeout) => Err(ConnectorError::Timeout),
        result = future => result,
    }
}

pub(crate) async fn postgres(pool: &sqlx::PgPool, pid: i32) {
    let result = tokio::time::timeout(Duration::from_secs(1), async {
        // A fresh connection avoids deadlock when every pool slot is occupied.
        let mut connection = sqlx::PgConnection::connect_with(&pool.connect_options()).await?;
        let result = sqlx::query("SELECT pg_cancel_backend($1)")
            .bind(pid)
            .persistent(false)
            .execute(&mut connection)
            .await;
        let _ = connection.close_hard().await;
        result
    })
    .await;
    if !matches!(result, Ok(Ok(_))) {
        tracing::warn!("Postgres server-side cancellation failed; closing query connection");
    }
}
pub(crate) async fn mysql(pool: &sqlx::MySqlPool, id: u64) {
    let result = tokio::time::timeout(Duration::from_secs(1), async {
        let mut connection = sqlx::MySqlConnection::connect_with(&pool.connect_options()).await?;
        let result = sqlx::query(&format!("KILL QUERY {id}"))
            .persistent(false)
            .execute(&mut connection)
            .await;
        let _ = connection.close_hard().await;
        result
    })
    .await;
    if !matches!(result, Ok(Ok(_))) {
        tracing::warn!("MySQL server-side cancellation failed; closing query connection");
    }
}

pub(crate) fn error(error: sqlx::Error) -> ConnectorError {
    match error {
        sqlx::Error::PoolTimedOut => ConnectorError::PoolExhausted,
        sqlx::Error::Database(ref db) => {
            if let Some(pg) = db.try_downcast_ref::<sqlx::postgres::PgDatabaseError>() {
                return database_code(pg.code(), pg.message());
            }
            if let Some(mysql) = db.try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>() {
                return database_code(&mysql.number().to_string(), mysql.message());
            }
            ConnectorError::Database(db.message().to_owned())
        }
        sqlx::Error::Io(_) | sqlx::Error::Tls(_) | sqlx::Error::PoolClosed => {
            ConnectorError::Connect(error.to_string())
        }
        _ => ConnectorError::Database(error.to_string()),
    }
}
fn database_code(code: &str, message: &str) -> ConnectorError {
    match code {
        "57014" | "3024" | "1969" => ConnectorError::Timeout,
        "1317" => ConnectorError::Cancelled,
        "1205" => ConnectorError::Database(format!("lock wait timeout: {message}")),
        _ => ConnectorError::Database(message.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn database_errors_have_stable_categories() {
        for code in ["57014", "3024", "1969"] {
            assert!(matches!(
                database_code(code, "timeout"),
                ConnectorError::Timeout
            ));
        }
        assert!(matches!(
            database_code("1317", "interrupted"),
            ConnectorError::Cancelled
        ));
        assert!(
            matches!(database_code("1205", "wait"), ConnectorError::Database(message) if message.contains("lock wait timeout"))
        );
        assert!(matches!(
            error(sqlx::Error::PoolTimedOut),
            ConnectorError::PoolExhausted
        ));
    }
    #[tokio::test]
    async fn pre_cancel_and_hard_timeout_win() {
        let token = CancellationToken::new();
        token.cancel();
        assert!(matches!(
            race(async { Ok(1) }, &token, Duration::from_secs(1)).await,
            Err(ConnectorError::Cancelled)
        ));
        assert!(matches!(
            race(
                std::future::pending::<Result<(), ConnectorError>>(),
                &CancellationToken::new(),
                Duration::from_millis(1)
            )
            .await,
            Err(ConnectorError::Timeout)
        ));
    }
}
