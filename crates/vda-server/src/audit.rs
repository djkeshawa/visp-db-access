//! Durable query intent and bounded asynchronous delivery of auxiliary audit events.
use chrono::Utc;
use serde_json::Value;
use std::{net::IpAddr, time::Duration};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Query identity and SQL captured synchronously before target I/O.
#[derive(Debug, Clone)]
pub struct History {
    pub id: Uuid,
    pub cluster_id: Uuid,
    pub cluster_name: String,
    pub user_id: Uuid,
    pub user_email: String,
    pub sql: String,
    pub verdict: String,
    pub status: String,
    pub row_count: Option<i64>,
    pub elapsed_ms: Option<i64>,
    pub error: Option<String>,
}
/// An append-only audit event. Query SQL lives durably in history, never in the queue.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Event {
    pub id: Uuid,
    pub actor_id: Option<Uuid>,
    pub action: String,
    pub target_type: Option<String>,
    pub target_id: Option<Uuid>,
    pub ip: Option<IpAddr>,
    pub details: Value,
    pub created_at: chrono::DateTime<Utc>,
}
/// Write an audit event using the same connection/transaction as its mutation.
pub async fn persist_on(
    connection: &mut sqlx::PgConnection,
    event: &Event,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO audit_log(id,actor_id,action,target_type,target_id,ip,details,created_at) VALUES
         ($1,$2,$3,$4,$5,$6::text::inet,$7,$8) ON CONFLICT(id) DO NOTHING",
    )
    .bind(event.id)
    .bind(event.actor_id)
    .bind(&event.action)
    .bind(&event.target_type)
    .bind(event.target_id)
    .bind(event.ip.map(|ip| ip.to_string()))
    .bind(&event.details)
    .bind(event.created_at)
    .execute(connection)
    .await?;
    Ok(())
}
/// Persist one idempotent event synchronously.
pub async fn persist(db: &sqlx::PgPool, event: &Event) -> Result<(), sqlx::Error> {
    persist_on(&mut *db.acquire().await?, event).await
}
/// Commit query history and its audit intent before any target operation.
pub async fn begin_query(
    db: &sqlx::PgPool,
    history: &History,
    event: &Event,
) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query(
        "INSERT INTO query_history(id,cluster_id,cluster_ref,cluster_name,user_id,user_email,
         sql,verdict,status,row_count,elapsed_ms,error,created_at,ip) VALUES ($1,$2,(SELECT id FROM clusters
         WHERE id=$2),$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13::text::inet)",
    )
    .bind(history.id)
.bind(history.cluster_id)
.bind(&history.cluster_name)
.bind(history.user_id)
    .bind(&history.user_email)
.bind(&history.sql)
.bind(&history.verdict)
.bind(&history.status)
    .bind(history.row_count)
.bind(history.elapsed_ms)
.bind(&history.error)
.bind(event.created_at)
    .bind(event.ip.map(|ip| ip.to_string()))
.execute(&mut *tx).await?;
    persist_on(&mut tx, event).await?;
    tx.commit().await
}
/// Retry transport failures and transaction serialization/deadlock failures only.
pub fn transient(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Io(_)
        | sqlx::Error::Tls(_)
        | sqlx::Error::PoolTimedOut
        | sqlx::Error::PoolClosed => true,
        sqlx::Error::Database(db) => db.code().is_some_and(|code| {
            code.starts_with("08")
                || matches!(
                    code.as_ref(),
                    "40001" | "40P01" | "57P01" | "57P02" | "57P03"
                )
        }),
        _ => false,
    }
}
async fn deliver(db: &sqlx::PgPool, event: &Event) {
    let mut backoff = Duration::from_millis(50);
    for attempt in 0..6 {
        match persist(db, event).await {
            Ok(()) => return,
            Err(error) if transient(&error) && attempt < 5 => {
                tracing::warn!(event_id=%event.id, %error, attempt, "transient audit persistence failure");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(1));
            }
            Err(error) => {
                tracing::error!(audit_dead_letter=?event, %error, "audit_dead_letter");
                return;
            }
        }
    }
}
/// Close on shutdown and bound the entire drain, including in-flight retries, to five seconds.
pub async fn writer(
    db: sqlx::PgPool,
    mut receiver: mpsc::Receiver<Event>,
    stop: CancellationToken,
) {
    let mut deadline = None;
    loop {
        if stop.is_cancelled() && deadline.is_none() {
            receiver.close();
            deadline = Some(tokio::time::Instant::now() + Duration::from_secs(5));
        }
        let event = tokio::select! {
            event = receiver.recv() => event,
            _ = stop.cancelled(), if deadline.is_none() => {
                receiver.close();
                deadline = Some(tokio::time::Instant::now() + Duration::from_secs(5));
                receiver.recv().await
            }
        };
        let Some(event) = event else { break };
        let delivery = deliver(&db, &event);
        tokio::pin!(delivery);
        let finished = if let Some(end) = deadline {
            tokio::time::timeout_at(end, &mut delivery).await.is_ok()
        } else {
            tokio::select! {
                _ = &mut delivery => true,
                _ = stop.cancelled() => {
                    receiver.close();
                    let end = tokio::time::Instant::now() + Duration::from_secs(5);
                    deadline = Some(end);
                    tokio::time::timeout_at(end, &mut delivery).await.is_ok()
                }
            }
        };
        if !finished {
            tracing::error!(audit_dead_letter=?event, "audit_dead_letter: shutdown deadline");
            while let Ok(event) = receiver.try_recv() {
                tracing::error!(audit_dead_letter=?event, "audit_dead_letter: shutdown deadline");
            }
            break;
        }
    }
}
