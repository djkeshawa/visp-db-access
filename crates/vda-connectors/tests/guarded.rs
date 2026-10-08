//! Opt-in tests against disposable database instances.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
use serde_json::{json, Value};
use sqlx::{mysql::MySqlPoolOptions, postgres::PgPoolOptions};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;
use vda_connectors::{ConnectorError, Engine, ExecLimits, TargetPool};

async fn pool(engine: Engine) -> Option<TargetPool> {
    let variable = match engine {
        Engine::Postgres => "VDA_TEST_PG_URL",
        Engine::Mysql => "VDA_TEST_MYSQL_URL",
    };
    let Ok(url) = std::env::var(variable) else {
        eprintln!("Skipping {engine:?}: {variable} is unset");
        return None;
    };
    // A single slot catches cancellation that mistakenly tries to use the busy pool.
    Some(match engine {
        Engine::Postgres => TargetPool::Postgres(
            PgPoolOptions::new()
                .max_connections(1)
                .acquire_timeout(Duration::from_secs(5))
                .after_connect(|connection, _| {
                    Box::pin(async move {
                        sqlx::query("SET default_transaction_read_only = on")
                            .execute(&mut *connection)
                            .await?;
                        sqlx::query("SET TIME ZONE 'UTC'")
                            .execute(connection)
                            .await?;
                        Ok(())
                    })
                })
                .connect(&url)
                .await
                .expect("connect to disposable Postgres"),
        ),
        Engine::Mysql => TargetPool::Mysql(
            MySqlPoolOptions::new()
                .max_connections(1)
                .acquire_timeout(Duration::from_secs(5))
                .connect(&url)
                .await
                .expect("connect to disposable MySQL"),
        ),
    })
}
fn limits() -> ExecLimits {
    ExecLimits {
        statement_timeout: Duration::from_secs(3),
        lock_timeout: Duration::from_millis(300),
        max_rows: 10,
        max_bytes: 1024 * 1024,
    }
}
async fn read(pool: &TargetPool, sql: &str) -> vda_connectors::QueryResult {
    pool.execute_read(sql, &limits(), CancellationToken::new())
        .await
        .unwrap()
}
async fn write(
    pool: &TargetPool,
    sql: &str,
    cap: u64,
) -> Result<vda_connectors::QueryResult, ConnectorError> {
    pool.execute_write(sql, &limits(), cap, CancellationToken::new())
        .await
}
fn table() -> String {
    format!("vda_test_{}", uuid::Uuid::new_v4().simple())
}
async fn create(pool: &TargetPool, name: &str) {
    write(
        pool,
        &format!("CREATE TABLE {name} (id BIGINT PRIMARY KEY, value VARCHAR(100) NULL)"),
        0,
    )
    .await
    .unwrap();
}
async fn drop_table(pool: &TargetPool, name: &str) {
    write(pool, &format!("DROP TABLE {name}"), 0).await.unwrap();
}

async fn truncation(engine: Engine) {
    let Some(pool) = pool(engine).await else {
        return;
    };
    let sql = "SELECT 1 AS n UNION ALL SELECT 2 UNION ALL SELECT 3";
    let mut budget = limits();
    budget.max_rows = 2;
    let result = pool
        .execute_read(sql, &budget, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(result.row_count, 2);
    assert!(result.truncated);
    assert_eq!(result.columns.first().unwrap().name, "n");
    budget.max_rows = 3;
    assert!(
        !pool
            .execute_read(sql, &budget, CancellationToken::new())
            .await
            .unwrap()
            .truncated
    );
    budget.max_rows = 0;
    assert_eq!(
        pool.execute_read(sql, &budget, CancellationToken::new())
            .await
            .unwrap()
            .row_count,
        0
    );
    budget.max_rows = 10;
    budget.max_bytes = 2;
    let result = pool
        .execute_read(
            "SELECT 'long text' AS value",
            &budget,
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(result.truncated);
    assert!(result.rows.is_empty());
    assert_eq!(
        read(&pool, "SELECT 42")
            .await
            .rows
            .first()
            .unwrap()
            .first()
            .unwrap(),
        &json!(42)
    );
    let empty = read(&pool, "SELECT 1 AS empty_column WHERE 1=0").await;
    assert_eq!(empty.columns.first().unwrap().name, "empty_column");
    assert_eq!(empty.row_count, 0);
    pool.close().await;
}
async fn rollback_and_read_only(engine: Engine) {
    let Some(pool) = pool(engine).await else {
        return;
    };
    let name = table();
    create(&pool, &name).await;
    let insert =
        format!("INSERT INTO {name} (id, value) VALUES (1, 'first'), (2, NULL), (3, 'third')");
    assert!(matches!(
        pool.execute_read(&insert, &limits(), CancellationToken::new())
            .await,
        Err(ConnectorError::Database(_))
    ));
    assert!(matches!(
        write(&pool, &insert, 2).await,
        Err(ConnectorError::TooManyAffectedRows {
            actual: 3,
            limit: 2
        })
    ));
    assert_eq!(
        read(&pool, &format!("SELECT COUNT(*) FROM {name}"))
            .await
            .rows
            .first()
            .unwrap()
            .first()
            .unwrap(),
        &json!(0)
    );
    let result = write(&pool, &insert, 3).await.unwrap();
    assert_eq!(result.affected_rows, Some(3));
    let schema = pool.schema().await.unwrap();
    let catalog = schema
        .schemas
        .iter()
        .flat_map(|s| &s.tables)
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("table {name} absent: {schema:?}"));
    assert_eq!(catalog.columns.len(), 2);
    assert!(catalog.columns.first().unwrap().is_primary_key);
    assert!(!catalog.columns.first().unwrap().nullable);
    assert!(catalog.columns.last().unwrap().nullable);
    drop_table(&pool, &name).await;
    pool.close().await;
}
async fn cancellation(engine: Engine) {
    let Some(pool) = pool(engine).await else {
        return;
    };
    let slow = match engine {
        Engine::Postgres => "SELECT pg_sleep(20)",
        Engine::Mysql => "SELECT SLEEP(20)",
    };
    let token = CancellationToken::new();
    let trigger = token.clone();
    let mut budget = limits();
    budget.statement_timeout = Duration::from_secs(30);
    let start = Instant::now();
    let query = pool.execute_read(slow, &budget, token);
    let cancel = async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        trigger.cancel();
    };
    let (result, ()) = tokio::join!(query, cancel);
    assert!(matches!(result, Err(ConnectorError::Cancelled)));
    assert!(start.elapsed() < Duration::from_secs(3));
    assert!(pool.health().await.ok);
    let query_pool = pool.clone();
    let task = tokio::spawn(async move {
        query_pool
            .execute_read(slow, &budget, CancellationToken::new())
            .await
    });
    tokio::time::sleep(Duration::from_millis(150)).await;
    task.abort();
    let _ = task.await;
    assert!(
        pool.health().await.ok,
        "dropping a future must release its connection"
    );
    pool.close().await;
}
async fn health_and_explain(engine: Engine) {
    let Some(pool) = pool(engine).await else {
        return;
    };
    let health = pool.health().await;
    assert!(health.ok, "{health:?}");
    assert!(health.latency_ms.is_some());
    assert!(health.server_version.is_some());
    assert!(health.max_connections.is_some());
    assert!(health.active_connections.is_some(), "{health:?}");
    assert!(health.is_replica.is_some(), "{health:?}");
    let name = table();
    create(&pool, &name).await;
    let cost = pool
        .explain_cost(&format!("SELECT * FROM {name}"), &limits())
        .await
        .unwrap();
    assert!(cost.is_some_and(|cost| cost >= 0.0));
    drop_table(&pool, &name).await;
    pool.close().await;
}
#[tokio::test]
async fn postgres_truncation() {
    truncation(Engine::Postgres).await;
}
#[tokio::test]
async fn mysql_truncation() {
    truncation(Engine::Mysql).await;
}
#[tokio::test]
async fn postgres_rollback_read_only_schema() {
    rollback_and_read_only(Engine::Postgres).await;
}
#[tokio::test]
async fn mysql_rollback_read_only_schema() {
    rollback_and_read_only(Engine::Mysql).await;
}
#[tokio::test]
async fn postgres_cancellation() {
    cancellation(Engine::Postgres).await;
}
#[tokio::test]
async fn mysql_cancellation() {
    cancellation(Engine::Mysql).await;
}
#[tokio::test]
async fn postgres_health_and_explain() {
    health_and_explain(Engine::Postgres).await;
}
#[tokio::test]
async fn mysql_health_and_explain() {
    health_and_explain(Engine::Mysql).await;
}

#[tokio::test]
async fn postgres_statement_timeout() {
    let Some(pool) = pool(Engine::Postgres).await else {
        return;
    };
    let mut budget = limits();
    budget.statement_timeout = Duration::from_millis(100);
    assert!(matches!(
        pool.execute_read("SELECT pg_sleep(5)", &budget, CancellationToken::new())
            .await,
        Err(ConnectorError::Timeout)
    ));
    assert!(pool.health().await.ok);
    pool.close().await;
}
#[tokio::test]
async fn mysql_statement_timeout() {
    let Some(pool) = pool(Engine::Mysql).await else {
        return;
    };
    let mut budget = limits();
    budget.statement_timeout = Duration::from_millis(100);
    assert!(matches!(
        pool.execute_read(
            "SELECT 1 WHERE SLEEP(5) = 0",
            &budget,
            CancellationToken::new()
        )
        .await,
        Err(ConnectorError::Timeout)
    ));
    assert!(pool.health().await.ok);
    pool.close().await;
}
#[tokio::test]
async fn postgres_returning_counts_all_affected_rows_with_bounded_storage() {
    let Some(pool) = pool(Engine::Postgres).await else {
        return;
    };
    let name = table();
    create(&pool, &name).await;
    let sql = format!("INSERT INTO {name} (id) SELECT generate_series(1, 20) RETURNING id");
    let mut budget = limits();
    budget.max_rows = 2;
    let result = pool
        .execute_write(&sql, &budget, 20, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(result.affected_rows, Some(20));
    assert_eq!(result.row_count, 2);
    assert!(result.truncated);
    let sql = format!("UPDATE {name} SET value='changed' RETURNING id");
    assert!(matches!(
        pool.execute_write(&sql, &budget, 19, CancellationToken::new())
            .await,
        Err(ConnectorError::TooManyAffectedRows {
            actual: 20,
            limit: 19
        })
    ));
    assert_eq!(
        read(
            &pool,
            &format!("SELECT count(*) FROM {name} WHERE value IS NOT NULL")
        )
        .await
        .rows
        .first()
        .unwrap()
        .first()
        .unwrap(),
        &json!(0)
    );
    drop_table(&pool, &name).await;
    pool.close().await;
}
#[tokio::test]
async fn postgres_type_round_trip() {
    let Some(pool) = pool(Engine::Postgres).await else {
        return;
    };
    let result = read(&pool, r#"SELECT NULL::text, true, 12::int2, 42::int4, 9007199254740993::int8, 'NaN'::float8, 1.5::float4, 1234567890123456789012345678901234567890.1234::numeric, 'hello'::text, '550e8400-e29b-41d4-a716-446655440000'::uuid, '2024-01-02'::date, '03:04:05'::time, '2024-01-02 03:04:05'::timestamp, '2024-01-02 03:04:05+00'::timestamptz, '1 month 2 days 3 seconds'::interval, '{"a":1}'::jsonb, decode('616263','hex'), ARRAY[1,NULL,9007199254740993]::bigint[], ARRAY['hello',NULL]::text[], '192.168.1.0/24'::cidr, '192.168.1.1'::inet, '08:00:2b:01:02:03'::macaddr, ARRAY[1.2300,NULL]::numeric[], ARRAY[[1,2],[3,NULL]]::bigint[]"#).await;
    let values = result.rows.first().unwrap();
    for (index, expected) in [
        (0, Value::Null),
        (1, json!(true)),
        (2, json!(12)),
        (3, json!(42)),
        (4, json!("9007199254740993")),
        (5, json!("NaN")),
        (6, json!(1.5)),
        (7, json!("1234567890123456789012345678901234567890.1234")),
        (8, json!("hello")),
        (9, json!("550e8400-e29b-41d4-a716-446655440000")),
        (10, json!("2024-01-02")),
        (11, json!("03:04:05")),
        (12, json!("2024-01-02T03:04:05")),
        (13, json!("2024-01-02T03:04:05+00:00")),
        (14, json!("P1M2DT3.000000S")),
        (15, json!({"a":1})),
        (16, json!("base64:YWJj")),
        (17, json!([1, null, "9007199254740993"])),
        (18, json!(["hello", null])),
        (19, json!("192.168.1.0/24")),
        (20, json!("192.168.1.1")),
        (21, json!("08:00:2b:01:02:03")),
        (22, json!(["1.2300", null])),
        (23, json!([[1, 2], [3, null]])),
    ] {
        assert_eq!(values.get(index), Some(&expected), "column {index}");
    }
    pool.close().await;
}
#[tokio::test]
async fn mysql_type_round_trip() {
    let Some(pool) = pool(Engine::Mysql).await else {
        return;
    };
    let mariadb = pool
        .health()
        .await
        .server_version
        .is_some_and(|v| v.contains("MariaDB"));
    let result = read(&pool, r#"SELECT NULL, CAST(-42 AS SIGNED), CAST(18446744073709551615 AS UNSIGNED), CAST(1234567890123456789012345678901234567890.1234 AS DECIMAL(44,4)), 'hello', CAST('2024-01-02' AS DATE), CAST('03:04:05' AS TIME), CAST('2024-01-02 03:04:05' AS DATETIME), JSON_OBJECT('a',1), CAST('abc' AS BINARY)"#).await;
    let values = result.rows.first().unwrap();
    for (index, expected) in [
        (0, Value::Null),
        (1, json!(-42)),
        (2, json!("18446744073709551615")),
        (3, json!("1234567890123456789012345678901234567890.1234")),
        (4, json!("hello")),
        (5, json!("2024-01-02")),
        (6, json!("03:04:05")),
        (7, json!("2024-01-02T03:04:05")),
        (
            8,
            if mariadb {
                json!("{\"a\": 1}")
            } else {
                json!({"a":1})
            },
        ),
        (9, json!("base64:YWJj")),
    ] {
        assert_eq!(values.get(index), Some(&expected), "column {index}");
    }
    pool.close().await;
}

#[tokio::test]
async fn mysql_stored_types_and_guarded_literals() {
    let Some(pool) = pool(Engine::Mysql).await else {
        return;
    };
    let mariadb = pool
        .health()
        .await
        .server_version
        .is_some_and(|v| v.contains("MariaDB"));
    let name = table();
    write(&pool, &format!("CREATE TABLE {name} (flag TINYINT(1), bits BIT(9), amount DECIMAL(44,4), dt DATETIME(6), ts TIMESTAMP(6), negative TIME(6), duration TIME(6), yr YEAR, doc JSON, choice ENUM('a','b'), choices SET('a','b'), bytes BLOB, uuid BINARY(16), shape GEOMETRY, huge BIGINT UNSIGNED, emoji VARCHAR(40)) CHARACTER SET utf8mb4"), 0).await.unwrap();
    write(&pool, &format!(r#"INSERT INTO {name} VALUES (1,b'100000001',1234567890123456789012345678901234567890.1234,'2024-01-02 03:04:05.123456','2024-01-02 03:04:05.123456','-30:04:05.123400','48:00:00',2024,JSON_OBJECT('a',1),'b','a,b',X'616263',UNHEX('550e8400e29b41d4a716446655440000'),ST_GeomFromText('POINT(1 2)'),18446744073709551615,'hello 😀')"#), 1).await.unwrap();
    let result = read(&pool, &format!("SELECT * FROM {name}")).await;
    let values = result.rows.first().unwrap();
    for (index, expected) in [
        (0, json!(true)),
        (1, json!("base64:AQE=")),
        (2, json!("1234567890123456789012345678901234567890.1234")),
        (3, json!("2024-01-02T03:04:05.123456")),
        (4, json!("2024-01-02T03:04:05.123456+00:00")),
        (5, json!("-PT30H4M5.1234S")),
        (6, json!("PT48H0M0S")),
        (7, json!(2024)),
        (
            8,
            if mariadb {
                json!("base64:eyJhIjogMX0=")
            } else {
                json!({"a":1})
            },
        ),
        (9, json!("b")),
        (10, json!("a,b")),
        (11, json!("base64:YWJj")),
        (12, json!("base64:VQ6EAOKbQdSnFkRmVUQAAA==")),
        (13, json!("<unsupported type: GEOMETRY>")),
        (14, json!("18446744073709551615")),
        (15, json!("hello 😀")),
    ] {
        assert_eq!(
            values.get(index),
            Some(&expected),
            "column {index} ({:?})",
            result.columns.get(index)
        );
    }
    drop_table(&pool, &name).await;
    let policy = vda_guard::GuardPolicy {
        max_rows: 10,
        allow_writes: true,
        allow_ddl: true,
        require_approval_for_writes: false,
        blocked_tables: vec![],
    };
    for (sql, expected) in [
        ("SELECT 7 # comment\n", json!(7)),
        ("SELECT 7 -- comment\n", json!(7)),
        (r"SELECT 'it\'s safe'", json!("it's safe")),
        (r#"SELECT "it's safe""#, json!("it's safe")),
        ("SELECT 7 AS `a``b`", json!(7)),
        (r"SELECT '\\' AS slash", json!("\\")),
        (r"SELECT '\a\f'", json!("af")),
        (r"SELECT '\%'", json!("\\%")),
        ("SELECT '/*!50000 literal */'", json!("/*!50000 literal */")),
        ("SELECT 'hello 😀'", json!("hello 😀")),
    ] {
        let analysis = vda_guard::analyze(
            sql,
            vda_guard::Dialect::MySql,
            vda_guard::AccessLevel::Read,
            &policy,
        );
        assert_eq!(analysis.verdict, vda_guard::Verdict::Allow, "{analysis:?}");
        let rewritten = analysis.rewritten_sql.unwrap();
        assert_eq!(
            read(&pool, &rewritten).await.rows[0][0],
            expected,
            "{sql} -> {rewritten}"
        );
    }
    // The server really executes versioned comments; the guard must see them first.
    assert_eq!(
        read(&pool, "SELECT /*!50000 7 */").await.rows[0][0],
        json!(7)
    );
    assert_eq!(
        vda_guard::analyze(
            "SELECT /*!50000 7 */",
            vda_guard::Dialect::MySql,
            vda_guard::AccessLevel::Admin,
            &policy
        )
        .verdict,
        vda_guard::Verdict::Deny
    );
    // Connection setup must remove syntax-changing defaults even for externally supplied pools.
    if let TargetPool::Mysql(inner) = &pool {
        sqlx::raw_sql("SET SESSION sql_mode='NO_BACKSLASH_ESCAPES,ANSI_QUOTES'")
            .execute(inner)
            .await
            .unwrap();
    }
    assert_eq!(read(&pool, r"SELECT '\\'").await.rows[0][0], json!("\\"));
    pool.close().await;
}

#[tokio::test]
async fn mysql_factory_health_replica_and_cancellation_discard() {
    let Ok(url) = std::env::var("VDA_TEST_MYSQL_URL") else {
        return;
    };
    let Ok(password) = std::env::var("VDA_TEST_MYSQL_PASSWORD") else {
        eprintln!("Skipping factory credential test: VDA_TEST_MYSQL_PASSWORD is unset");
        return;
    };
    let options: sqlx::mysql::MySqlConnectOptions = url.parse().unwrap();
    let spec = vda_connectors::ConnectionSpec {
        engine: Engine::Mysql,
        host: options.get_host().into(),
        port: options.get_port(),
        database: options.get_database().unwrap().into(),
        username: options.get_username().into(),
        // Live local fixture only; the normal integration URL remains configurable.
        password: secrecy::SecretString::from(password),
        tls: vda_connectors::TlsMode::Disable,
        ca_cert_pem: None,
        application_name: "vda-live".into(),
    };
    let pool = TargetPool::new_lazy(
        &spec,
        vda_connectors::PoolOptions {
            max_connections: 1,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(pool.health().await.ok);
    let before = read(
        &pool,
        "SELECT CONNECTION_ID(), @@session.transaction_read_only, @@global.read_only",
    )
    .await;
    assert_eq!(before.rows[0][1], json!(1));
    assert_eq!(
        pool.health().await.is_replica,
        Some(before.rows[0][2].as_i64().unwrap() != 0)
    );
    // Cancellation must discard the connection, never return it with a pending result/transaction.
    let token = CancellationToken::new();
    let trigger = token.clone();
    let mut budget = limits();
    budget.statement_timeout = Duration::from_secs(20);
    let query = pool.execute_read("SELECT SLEEP(10)", &budget, token);
    let cancel = async {
        tokio::time::sleep(Duration::from_millis(150)).await;
        trigger.cancel();
    };
    let (result, ()) = tokio::join!(query, cancel);
    assert!(matches!(result, Err(ConnectorError::Cancelled)));
    let after = read(&pool, "SELECT CONNECTION_ID()").await;
    assert_ne!(before.rows[0][0], after.rows[0][0]);
    // KILL QUERY used a fresh connection: the single execution slot stayed occupied.
    pool.close().await;
}

#[tokio::test]
async fn mysql_server_execution_timeout_really_interrupts_work() {
    let Some(pool) = pool(Engine::Mysql).await else {
        return;
    };
    let mut budget = limits();
    budget.statement_timeout = Duration::from_millis(100);
    let start = Instant::now();
    let sql="WITH RECURSIVE n AS (SELECT 1 AS id UNION ALL SELECT id+1 FROM n WHERE id<100) SELECT SUM(a.id+b.id+c.id+d.id) FROM n a CROSS JOIN n b CROSS JOIN n c CROSS JOIN n d";
    assert!(matches!(
        pool.execute_read(sql, &budget, CancellationToken::new())
            .await,
        Err(ConnectorError::Timeout)
    ));
    // The client hard deadline is statement timeout + 2s. This proves server-side
    // max_execution_time fired (3024), rather than only the fallback client timer.
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "elapsed {:?}",
        start.elapsed()
    );
    assert!(pool.health().await.ok);
    pool.close().await;
}
