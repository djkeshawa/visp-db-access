#![allow(clippy::unwrap_used)]
use secrecy::SecretString;
use std::time::Duration;
use vda_connectors::{ConnectionSpec, Engine, PoolOptions, TargetPool, TlsMode};

#[tokio::test]
async fn lazy_pools_do_not_connect_and_reject_invalid_limits() {
    for engine in [Engine::Postgres, Engine::Mysql] {
        let spec = ConnectionSpec {
            engine,
            host: "127.0.0.1".into(),
            port: 1,
            database: "unused".into(),
            username: "unused".into(),
            password: SecretString::from("secret"),
            tls: TlsMode::Disable,
            ca_cert_pem: None,
            application_name: "test".into(),
        };
        let pool = TargetPool::new_lazy(&spec, PoolOptions::default()).unwrap();
        assert_eq!(pool.engine(), engine);
        pool.close().await;
        assert!(TargetPool::new_lazy(
            &spec,
            PoolOptions {
                max_connections: 0,
                ..PoolOptions::default()
            }
        )
        .is_err());
        assert!(TargetPool::new_lazy(
            &spec,
            PoolOptions {
                connect_timeout: Duration::ZERO,
                ..PoolOptions::default()
            }
        )
        .is_err());
    }
}

#[tokio::test]
async fn handshake_timeout_is_bounded_by_connect_timeout() {
    for engine in [Engine::Postgres, Engine::Mysql] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (connection, _) = listener.accept().await.unwrap();
            std::future::pending::<()>().await;
            drop(connection);
        });
        let spec = ConnectionSpec {
            engine,
            host: "127.0.0.1".into(),
            port,
            database: "unused".into(),
            username: "unused".into(),
            password: SecretString::from("secret"),
            tls: TlsMode::Disable,
            ca_cert_pem: None,
            application_name: "test".into(),
        };
        let pool = TargetPool::new_lazy(
            &spec,
            PoolOptions {
                max_connections: 1,
                connect_timeout: Duration::from_millis(100),
                acquire_timeout: Duration::from_secs(5),
                ..PoolOptions::default()
            },
        )
        .unwrap();
        let start = std::time::Instant::now();
        let health = tokio::time::timeout(Duration::from_secs(1), pool.health())
            .await
            .unwrap();
        assert!(!health.ok);
        assert!(health.error.is_some());
        assert!(start.elapsed() < Duration::from_secs(1));
        pool.close().await;
        server.abort();
        let _ = server.await;
    }
}

#[tokio::test]
async fn connector_futures_can_run_on_multithreaded_servers() {
    fn assert_send<T: Send>(_: T) {}
    let budget = vda_connectors::ExecLimits {
        statement_timeout: Duration::from_secs(1),
        lock_timeout: Duration::from_millis(100),
        max_rows: 1,
        max_bytes: 100,
    };
    for engine in [Engine::Postgres, Engine::Mysql] {
        let spec = ConnectionSpec {
            engine,
            host: "127.0.0.1".into(),
            port: 1,
            database: "unused".into(),
            username: "unused".into(),
            password: SecretString::from("secret"),
            tls: TlsMode::Disable,
            ca_cert_pem: None,
            application_name: "test".into(),
        };
        let pool = TargetPool::new_lazy(&spec, PoolOptions::default()).unwrap();
        assert_send(pool.health());
        assert_send(pool.schema());
        assert_send(pool.explain_cost("SELECT 1", &budget));
        assert_send(pool.execute_read(
            "SELECT 1",
            &budget,
            tokio_util::sync::CancellationToken::new(),
        ));
        assert_send(pool.execute_write(
            "DELETE FROM unused",
            &budget,
            1,
            tokio_util::sync::CancellationToken::new(),
        ));
        assert_send(vda_connectors::test_connection(&spec));
        pool.close().await;
    }
}
