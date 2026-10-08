//! Leader-elected, bounded target health scheduler and retention maintenance.
use crate::{app::AppState, auth::User, db, error::ApiError, rbac};
use axum::{
    extract::{Path, Query, State},
    Extension, Json,
};
use chrono::{DateTime, Utc};
use futures::{stream, StreamExt};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Connection;
use uuid::Uuid;

/// Complete health wire format, with unknown values represented as null.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Health {
    pub status: String,
    pub latency_ms: Option<u64>,
    pub server_version: Option<String>,
    pub is_replica: Option<bool>,
    pub active_connections: Option<i64>,
    pub max_connections: Option<i64>,
    pub error: Option<String>,
    pub checked_at: Option<DateTime<Utc>>,
}
impl Health {
    /// Convert a connector probe into a policy-independent status.
    pub fn from_report(report: vda_connectors::HealthReport) -> Self {
        let busy = match (report.active_connections, report.max_connections) {
            (Some(a), Some(m)) if m > 0 => a as f64 / m as f64 > 0.85,
            _ => false,
        };
        let status = if !report.ok {
            "down"
        } else if report.latency_ms.is_some_and(|v| v > 500) || busy {
            "degraded"
        } else {
            "healthy"
        };
        Self {
            status: status.into(),
            latency_ms: report.latency_ms,
            server_version: report.server_version,
            is_replica: report.is_replica,
            active_connections: report.active_connections,
            max_connections: report.max_connections,
            error: report.error.map(|_| "Target database unavailable".into()),
            checked_at: Some(Utc::now()),
        }
    }
}
/// Public-health JSON projection for a cluster aliased as `c`.
pub const CURRENT_HEALTH:&str="COALESCE((SELECT
         jsonb_build_object('status',h.status,'latency_ms',h.latency_ms,
         'server_version',h.server_version,'is_replica',h.is_replica,
         'active_connections',h.details->'active_connections',
         'max_connections',h.details->'max_connections',
         'error',h.details->'error','checked_at',h.checked_at)
         FROM health_checks h WHERE h.cluster_id=c.id AND h.endpoint='primary' ORDER BY h.checked_at DESC,h.id
         DESC LIMIT
         1),'{\"status\":\"unknown\",\"latency_ms\":null,\"server_version\":null,
         \"is_replica\":null,\"active_connections\":null,\"max_connections\":null,
         \"error\":null,\"checked_at\":null}'::jsonb)";
/// Retrieve the latest primary health, or unknown for an unprobed cluster.
pub async fn current(db: &sqlx::PgPool, id: Uuid) -> Result<Health, ApiError> {
    let value: Value = sqlx::query_scalar(&format!(
        "SELECT {CURRENT_HEALTH} FROM clusters c WHERE c.id=$1"
    ))
    .bind(id)
    .fetch_one(db)
    .await?;
    serde_json::from_value(value).map_err(|_| ApiError::internal())
}
async fn store(
    db: &sqlx::PgPool,
    id: Uuid,
    endpoint: &str,
    health: &Health,
) -> Result<(), ApiError> {
    sqlx::query(
"INSERT INTO health_checks(cluster_id,endpoint,status,latency_ms,server_version,is_replica,details,checked_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
)
        .bind(id)
.bind(endpoint)
.bind(&health.status)
.bind(health.latency_ms.map(|v|v as i64))
.bind(&health.server_version)
.bind(health.is_replica)
.bind(json!({"active_connections":health.active_connections,"max_connections":health.max_connections,"error":health.error}))
.bind(health.checked_at.unwrap_or_else(Utc::now))
.execute(db).await?;
    Ok(())
}
/// Run and persist a probe against the primary and any configured replica.
pub async fn probe(state: &AppState, c: &db::ClusterRecord) -> Result<Health, ApiError> {
    let policy = db::policy(&state.db, c.id).await?;
    // Cached pools may establish new TCP connections: re-check DNS on every probe.
    let validated = crate::pools::validate_endpoints(state, c).await;
    let pools = match async {
        validated?;
        state.pools.get(state, c, &policy).await
    }
    .await
    {
        Ok(pools) => pools,
        Err(error) => {
            tracing::warn!(cluster_id=%c.id, %error, "target pool unavailable during health probe");
            let down = Health {
                status: "down".into(),
                latency_ms: None,
                server_version: None,
                is_replica: None,
                active_connections: None,
                max_connections: None,
                error: Some(if error.code == "credentials_unreadable" {
                    error.message.clone()
                } else {
                    "Target database unavailable".into()
                }),
                checked_at: Some(Utc::now()),
            };
            store(&state.db, c.id, "primary", &down).await?;
            if c.replica_host.is_some() {
                store(&state.db, c.id, "replica", &down).await?;
            }
            return Ok(down);
        }
    };
    let primary = Health::from_report(pools.primary.health().await);
    store(&state.db, c.id, "primary", &primary).await?;
    if let Some(replica) = &pools.replica {
        let health = Health::from_report(replica.health().await);
        store(&state.db, c.id, "replica", &health).await?;
    }
    Ok(primary)
}
#[derive(Debug, Deserialize)]
pub struct HealthQuery {
    pub hours: Option<i64>,
}
/// Return bounded historical primary health samples.
pub async fn history(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(id): Path<String>,
    Query(query): Query<HealthQuery>,
) -> Result<Json<Value>, ApiError> {
    let id = db::id(&id)?;
    rbac::require_level(&state.db, &user, id, vda_guard::AccessLevel::Read).await?;
    let hours = query.hours.unwrap_or(24);
    if !(1..=168).contains(&hours) {
        return Err(ApiError::validation("Hours must be 1..=168"));
    }
    let rows:Vec<Value>=sqlx::query_scalar(
"SELECT jsonb_build_object('status',status,'latency_ms',latency_ms,'checked_at',checked_at) FROM
         health_checks WHERE cluster_id=$1 AND endpoint='primary' AND checked_at>now()-($2*interval '1 hour')
         ORDER BY checked_at DESC LIMIT 20160",
)
.bind(id)
.bind(hours)
.fetch_all(&state.db).await?;
    Ok(Json(
        json!({"current":current(&state.db,id).await?,"history":rows}),
    ))
}
/// On-demand probe restricted to cluster administrators.
pub async fn check(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Extension(ip): Extension<crate::network::ClientIp>,
    Path(id): Path<String>,
) -> Result<Json<Health>, ApiError> {
    let cluster = rbac::require_level(
        &state.db,
        &user,
        db::id(&id)?,
        vda_guard::AccessLevel::Admin,
    )
    .await?;
    crate::network::require_cluster(&state, cluster.id, ip.0).await?;
    Ok(Json(probe(&state, &cluster).await?))
}
/// Run maintenance only while holding a session advisory lock on a dedicated connection.
pub async fn scheduler(state: AppState) {
    let mut connection: Option<sqlx::PgConnection> = None;
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {_=state.stop.cancelled()=>break,_=interval.tick()=>{}}
        if connection.is_none() {
            let Some(url) = &state.config.database_url else {
                break;
            };
            match sqlx::PgConnection::connect(url.expose_secret()).await {
                Ok(c) => connection = Some(c),
                Err(e) => {
                    tracing::warn!(error=%e,"health leader connection failed");
                    continue;
                }
            }
        }
        let Some(conn) = connection.as_mut() else {
            continue;
        };
        let leader: Result<bool, sqlx::Error> =
            sqlx::query_scalar("SELECT pg_try_advisory_lock(762104221)")
                .fetch_one(&mut *conn)
                .await;
        match leader {
            Ok(false) => continue,
            Err(e) => {
                tracing::warn!(error=%e,"health leadership lost");
                connection = None;
                continue;
            }
            Ok(true) => {}
        }
        // Release each tick: advisory locks are reentrant, so repeatedly acquiring a
        // held lock without unlocking would stack lock counts indefinitely.
        if let Err(e) = cycle(&state).await {
            tracing::error!(error=%e,"health maintenance failed");
        }
        if sqlx::query("SELECT pg_advisory_unlock(762104221)")
            .execute(&mut *conn)
            .await
            .is_err()
        {
            connection = None;
        }
    }
    if let Some(conn) = connection {
        let _ = conn.close().await;
    }
}
async fn cycle(state: &AppState) -> Result<(), ApiError> {
    let clusters: Vec<db::ClusterRecord> = sqlx::query_as("SELECT * FROM clusters")
        .fetch_all(&state.db)
        .await?;
    let probes = stream::iter(clusters)
        .map(|c| async move {
            if let Err(error) = probe(state, &c).await {
                tracing::warn!(cluster_id=%c.id,error=%error,"cluster probe failed");
            }
        })
        .buffer_unordered(16)
        .collect::<()>();
    tokio::select! {_=state.stop.cancelled()=>return Ok(()),_=probes=>{}}
    sqlx::query("DELETE FROM health_checks WHERE checked_at<now()-interval '7 days'")
        .execute(&state.db)
        .await?;
    sqlx::query("DELETE FROM sessions WHERE expires_at<=now() OR absolute_expires_at<=now()")
        .execute(&state.db)
        .await?;
    recover_executions(&state.db).await?;
    Ok(())
}

/// Recover abandoned executions without expiring in-flight approval claims.
pub async fn recover_executions(db: &sqlx::PgPool) -> Result<(), ApiError> {
    sqlx::query(
        "UPDATE approvals SET status='expired'
        WHERE status IN ('pending','approved') AND expires_at<=now()",
    )
    .execute(db)
    .await?;
    sqlx::query(
        "UPDATE approvals a SET
         status='failed',error='execution outcome unknown (node crashed or lost)',executed_at=now()
         WHERE a.status='executing' AND
         COALESCE(a.execution_started_at,a.reviewed_at,a.created_at)< now() -
         ((a.execution_timeout_ms+300000)*interval '1 millisecond')",
    )
    .execute(db)
    .await?;
    // Policies cap statements at ten minutes. Use that maximum plus five minutes
    // so configuration changes/deletions cannot prematurely retire live queries.
    sqlx::query(
"UPDATE query_history SET status='unknown',error='execution outcome unknown (node crashed or lost)'
         WHERE status='running' AND created_at<now()-interval '15 minutes'",
)
.execute(db).await?;
    Ok(())
}
