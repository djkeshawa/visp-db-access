//! Guarded execution against target databases (Postgres, MySQL).
//!
//! INTERFACE CONTRACT: the public types and `TargetPool` methods below are
//! consumed by `vda-server`. Keep them stable; add fields/methods only.
//! This crate does NOT decide whether SQL is safe — `vda-guard` does. It
//! enforces runtime limits: read-only transactions, timeouts, row/byte caps,
//! affected-row caps, and server-side cancellation.

mod cancel;
mod decode;
mod limits;
mod mysql;
mod postgres;
mod schema;
mod tls;

use std::time::Duration;

use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

/// Database dialect and driver for a target endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Engine {
    /// PostgreSQL target.
    Postgres,
    /// MySQL target.
    Mysql,
}

/// Transport encryption and server identity verification policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TlsMode {
    /// Use plaintext transport.
    Disable,
    /// Try TLS and permit fallback to plaintext.
    Prefer,
    /// Require TLS without CA or hostname verification.
    Require,
    /// Require TLS and validate the CA chain and hostname.
    VerifyFull,
}

/// Target credentials and transport settings; passwords remain secret in Debug output.
#[derive(Debug, Clone)]
pub struct ConnectionSpec {
    /// Target database driver.
    pub engine: Engine,
    /// DNS name or IP address of the target.
    pub host: String,
    /// Target TCP port.
    pub port: u16,
    /// Database selected on connection.
    pub database: String,
    /// Database login role.
    pub username: String,
    /// Database password; exposed only when constructing driver options.
    pub password: SecretString,
    /// Encryption and server identity policy.
    pub tls: TlsMode,
    /// Optional PEM CA bundle for `VerifyFull`.
    pub ca_cert_pem: Option<String>,
    /// PostgreSQL activity label; MySQL stores it in `@vda_application_name`.
    pub application_name: String,
}

/// Pool capacity and connection deadlines. Pools keep zero minimum idle connections.
#[derive(Debug, Clone, Copy)]
pub struct PoolOptions {
    /// Maximum checked-out and idle connections; must be positive.
    pub max_connections: u32,
    /// Maximum acquisition deadline, including connection setup.
    pub acquire_timeout: Duration,
    /// Connection setup deadline. SQLx uses the smaller of this and acquire_timeout
    /// for the whole acquisition, including queue wait and session initialization.
    pub connect_timeout: Duration,
    /// How long an unused connection may remain pooled.
    pub idle_timeout: Duration,
}

impl Default for PoolOptions {
    fn default() -> Self {
        Self {
            max_connections: 5,
            acquire_timeout: Duration::from_secs(5),
            connect_timeout: Duration::from_secs(5),
            idle_timeout: Duration::from_secs(300),
        }
    }
}

/// Execution budgets applied independently of SQL safety analysis.
#[derive(Debug, Clone, Copy)]
pub struct ExecLimits {
    /// Server statement deadline; client deadline adds two seconds.
    pub statement_timeout: Duration,
    /// Maximum lock wait (rounded up to whole seconds for MySQL).
    pub lock_timeout: Duration,
    /// Stop reading after this many rows; `truncated = true` if more existed.
    pub max_rows: usize,
    /// Maximum serialized row-array bytes; the overflow row is discarded.
    pub max_bytes: usize,
}

/// Result column metadata reported by the database driver.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Column {
    /// Database-reported name.
    pub name: String,
    /// Type name reported by the database.
    pub type_name: String,
}

/// Bounded query output and execution statistics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryResult {
    /// Ordered result columns, including metadata for empty results.
    pub columns: Vec<Column>,
    /// Values decoded to JSON. Unknown types fall back to their text form;
    /// binary → base64 string prefixed `base64:`.
    pub rows: Vec<Vec<serde_json::Value>>,
    /// Number of rows retained in this result.
    pub row_count: usize,
    /// Whether at least one output row was discarded due to a budget.
    pub truncated: bool,
    /// Full database affected count for writes; None for reads.
    pub affected_rows: Option<u64>,
    /// Wall-clock milliseconds, including acquisition and cleanup.
    pub elapsed_ms: u64,
}

/// Connectivity and best-effort diagnostic probes for a target.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HealthReport {
    /// Whether the connectivity probe completed within its deadline.
    pub ok: bool,
    /// Measured SELECT 1 latency, excluding pool acquisition.
    pub latency_ms: Option<u64>,
    /// Server version when the sub-probe succeeds.
    pub server_version: Option<String>,
    /// Recovery or replica state, inferred from engine-specific probes.
    pub is_replica: Option<bool>,
    /// Current connection count, if visible to the login role.
    pub active_connections: Option<i64>,
    /// Configured server connection limit, when available.
    pub max_connections: Option<i64>,
    /// Connectivity or overall deadline failure.
    pub error: Option<String>,
}

/// Catalog object type supported by schema introspection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableKind {
    /// Ordinary or partitioned table.
    Table,
    /// Logical view.
    View,
    /// PostgreSQL materialized view.
    MaterializedView,
}

/// Column definition from the target catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaColumn {
    /// Database-reported name.
    pub name: String,
    /// Catalog type spelling, including modifiers when available.
    pub data_type: String,
    /// Whether the catalog allows NULL in this column.
    pub nullable: bool,
    /// Whether this column participates in the primary key.
    pub is_primary_key: bool,
}

/// Table or view definition, including visible columns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaTable {
    /// Database-reported name.
    pub name: String,
    /// Table, view or materialized view classification.
    pub kind: TableKind,
    /// Nonnegative catalog row estimate; None when unavailable.
    pub row_estimate: Option<i64>,
    /// Columns in catalog ordinal order.
    pub columns: Vec<SchemaColumn>,
}

/// Named schema and its deterministically ordered tables.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaInfo {
    /// Database-reported name.
    pub name: String,
    /// Tables ordered deterministically by name.
    pub tables: Vec<SchemaTable>,
}

/// User catalog, bounded to 5,000 tables and 100,000 columns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaTree {
    /// Non-system schemas ordered deterministically by name.
    pub schemas: Vec<SchemaInfo>,
}

/// Stable connector failures consumed by the server query pipeline.
#[derive(Debug, thiserror::Error)]
pub enum ConnectorError {
    /// Transport, authentication or invalid pool configuration.
    #[error("connection failed: {0}")]
    Connect(String),
    /// Query or decoding failure reported by the database.
    #[error("database error: {0}")]
    Database(String),
    /// Server or client execution deadline exceeded.
    #[error("query timed out")]
    Timeout,
    /// Cancellation token or server query interruption.
    #[error("query cancelled")]
    Cancelled,
    /// Write exceeded its affected-row budget and was rolled back.
    #[error("affected rows {actual} exceed limit {limit}; transaction rolled back")]
    TooManyAffectedRows { actual: u64, limit: u64 },
    /// A pool connection was unavailable before its deadline.
    #[error("pool exhausted")]
    PoolExhausted,
}

/// A connection pool to one target endpoint (primary or replica).
#[derive(Debug, Clone)]
pub enum TargetPool {
    /// PostgreSQL driver pool.
    Postgres(sqlx::PgPool),
    /// MySQL driver pool.
    Mysql(sqlx::MySqlPool),
}

impl TargetPool {
    /// Lazily-connecting pool; does not fail if the DB is down.
    pub fn new_lazy(spec: &ConnectionSpec, opts: PoolOptions) -> Result<Self, ConnectorError> {
        if opts.max_connections == 0
            || opts.acquire_timeout.is_zero()
            || opts.connect_timeout.is_zero()
        {
            return Err(ConnectorError::Connect(
                "pool capacity and timeouts must be positive".into(),
            ));
        }
        Ok(match spec.engine {
            Engine::Postgres => Self::Postgres(postgres::pool(spec, opts)),
            Engine::Mysql => Self::Mysql(mysql::pool(spec, opts)),
        })
    }

    /// Database engine served by this pool.
    pub fn engine(&self) -> Engine {
        match self {
            Self::Postgres(_) => Engine::Postgres,
            Self::Mysql(_) => Engine::Mysql,
        }
    }

    /// Cheap probe: `SELECT 1` + version + replica flag + connection counts.
    /// Never returns Err; failures are reported in `HealthReport.error`.
    pub async fn health(&self) -> HealthReport {
        let mut report = empty_health();
        let probe = async {
            match self {
                Self::Postgres(pool) => postgres::health(pool, &mut report).await,
                Self::Mysql(pool) => mysql::health(pool, &mut report).await,
            }
        };
        match tokio::time::timeout(Duration::from_secs(5), probe).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                report.ok = false;
                report.error = Some(error.to_string());
            }
            Err(_) => {
                report.ok = false;
                report.error = Some("health probe timed out after 5 seconds".into());
            }
        }
        report
    }

    /// Estimated total cost from `EXPLAIN` (no execution). `None` if unavailable.
    pub async fn explain_cost(
        &self,
        sql: &str,
        limits: &ExecLimits,
    ) -> Result<Option<f64>, ConnectorError> {
        match self {
            Self::Postgres(pool) => postgres::explain(pool, sql, limits).await,
            Self::Mysql(pool) => mysql::explain(pool, sql, limits).await,
        }
    }

    /// Run a read-only statement in a READ ONLY transaction that is always rolled back.
    /// Cancelling `cancel` kills the query server-side and returns `Cancelled`.
    pub async fn execute_read(
        &self,
        sql: &str,
        limits: &ExecLimits,
        cancel: CancellationToken,
    ) -> Result<QueryResult, ConnectorError> {
        match self {
            Self::Postgres(pool) => postgres::execute(pool, sql, limits, None, cancel).await,
            Self::Mysql(pool) => mysql::execute(pool, sql, limits, None, cancel).await,
        }
    }

    /// Run a write in a transaction; roll back if affected rows > `max_affected_rows`.
    /// PostgreSQL RETURNING rows obey the read budgets without losing affected counts.
    /// Callers must reject implicit-commit SQL and nontransactional MySQL table writes
    /// when rollback guarantees are required.
    pub async fn execute_write(
        &self,
        sql: &str,
        limits: &ExecLimits,
        max_affected_rows: u64,
        cancel: CancellationToken,
    ) -> Result<QueryResult, ConnectorError> {
        match self {
            Self::Postgres(pool) => {
                postgres::execute(pool, sql, limits, Some(max_affected_rows), cancel).await
            }
            Self::Mysql(pool) => {
                mysql::execute(pool, sql, limits, Some(max_affected_rows), cancel).await
            }
        }
    }

    /// Introspect user schemas (excludes system schemas); MySQL uses the current database.
    pub async fn schema(&self) -> Result<SchemaTree, ConnectorError> {
        schema::load(self).await
    }

    /// Close idle connections and wait for active connections to be released.
    pub async fn close(&self) {
        match self {
            Self::Postgres(p) => p.close().await,
            Self::Mysql(p) => p.close().await,
        }
    }
}

/// One-shot connection test with a fresh single-connection pool.
pub async fn test_connection(spec: &ConnectionSpec) -> HealthReport {
    let options = PoolOptions {
        max_connections: 1,
        acquire_timeout: Duration::from_secs(5),
        connect_timeout: Duration::from_secs(5),
        ..PoolOptions::default()
    };
    match TargetPool::new_lazy(spec, options) {
        Ok(pool) => {
            let report = pool.health().await;
            pool.close().await;
            report
        }
        Err(error) => {
            let mut report = empty_health();
            report.error = Some(error.to_string());
            report
        }
    }
}

fn empty_health() -> HealthReport {
    HealthReport {
        ok: false,
        latency_ms: None,
        server_version: None,
        is_replica: None,
        active_connections: None,
        max_connections: None,
        error: None,
    }
}
