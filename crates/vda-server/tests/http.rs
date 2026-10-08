#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    middleware,
    routing::post,
    Router,
};
use serde_json::Value;
use tower::ServiceExt;
use vda_server::{
    auth::csrf,
    error::{ApiError, Input},
};

#[tokio::test]
async fn csrf_rejects_missing_and_wrong_headers_with_contract_error() {
    let router = Router::new()
        .route("/", post(|| async { StatusCode::NO_CONTENT }))
        .layer(middleware::from_fn(csrf));
    for header in [None, Some("other"), Some("vda")] {
        let mut request = Request::post("/");
        if let Some(header) = header {
            request = request.header("x-requested-with", header);
        }
        let response = router
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        if header == Some("vda") {
            assert_eq!(response.status(), StatusCode::NO_CONTENT);
        } else {
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
            let body: Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 10000).await.unwrap())
                    .unwrap();
            assert_eq!(body["error"]["code"], "forbidden");
            assert!(body["error"]["details"].is_null());
        }
    }
}

#[tokio::test]
async fn error_and_json_rejection_have_exact_envelopes() {
    async fn handler(Input(_): Input<Value>) -> Result<StatusCode, ApiError> {
        Err(ApiError::validation("bad").details(serde_json::json!({"field":"sql"})))
    }
    let router = Router::new().route("/", post(handler));
    for (input, expected) in [
        (
            "invalid",
            serde_json::json!({"error":{"code":"validation","message":"Invalid JSON body","details":null}}),
        ),
        (
            "{}",
            serde_json::json!({"error":{"code":"validation","message":"bad","details":{"field":"sql"}}}),
        ),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::post("/")
                    .header("content-type", "application/json")
                    .body(Body::from(input))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 10000).await.unwrap()).unwrap();
        assert_eq!(body, expected);
    }
}

#[tokio::test]
async fn database_statement_errors_follow_the_query_contract() {
    use axum::response::IntoResponse;
    let error = ApiError::from(vda_connectors::ConnectorError::Database(
        "column does not exist".into(),
    ));
    let response = error.into_response();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 10000).await.unwrap()).unwrap();
    assert_eq!(body["error"]["message"], "column does not exist");
    let error = ApiError::from(vda_connectors::ConnectorError::Connect(
        "secret connection diagnostics".into(),
    ));
    assert_eq!(error.message, "Target database unavailable");
}

#[tokio::test]
async fn system_routes_and_login_rate_limits_need_no_target_database() {
    use clap::Parser;
    use vda_server::{
        app::{router, AppState},
        config::Cli,
        crypto::Crypto,
    };
    let db = sqlx::postgres::PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_millis(20))
        .connect_lazy("postgres://test@127.0.0.1:9/test")
        .unwrap();
    let config = Cli::parse_from(["test"]).config;
    let (state, writer) = AppState::new(db, config, Crypto::new(&[1; 32]), None)
        .await
        .unwrap();
    let app = router(state.clone());
    let response = app
        .clone()
        .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().contains_key("x-request-id"));
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert_eq!(to_bytes(response.into_body(), 1000).await.unwrap(), "ok");
    let response = app
        .clone()
        .oneshot(Request::get("/readyz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let response = app
        .oneshot(Request::get("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let ip = "203.0.113.10".parse().unwrap();
    for _ in 0..10 {
        state
            .check_login_rate(ip, "user@example.com")
            .await
            .unwrap();
    }
    assert_eq!(
        state
            .check_login_rate(ip, "user@example.com")
            .await
            .unwrap_err()
            .code,
        "rate_limited"
    );
    assert!(state
        .check_login_rate("203.0.113.11".parse().unwrap(), "user@example.com")
        .await
        .is_ok());
    for _ in 0..10 {
        state.login_failed("victim@example.com").await;
    }
    assert_eq!(
        state
            .check_account("victim@example.com")
            .await
            .unwrap_err()
            .code,
        "unauthenticated"
    );
    assert!(state.check_account("different@example.com").await.is_ok());
    state.login_succeeded("victim@example.com").await;
    assert!(state.check_account("victim@example.com").await.is_ok());
    state.stop.cancel();
    writer.await.unwrap();
    state.db.close().await;
}

#[tokio::test]
async fn argon2id_hashes_are_salted_and_verification_rejects_bad_passwords() {
    use vda_server::auth::{hash_password, verify_password};
    let password = "a-long-valid-password".to_owned();
    let a = hash_password(password.clone()).await.unwrap();
    let b = hash_password(password.clone()).await.unwrap();
    assert_ne!(a, b);
    assert!(a.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
    assert!(verify_password(password, a.clone()).await.unwrap());
    assert!(!verify_password("incorrect".into(), a).await.unwrap());
    assert!(hash_password("short".into()).await.is_err());
}

#[tokio::test]
async fn all_json_text_fields_and_keys_reject_controls_before_handler_runs() {
    async fn handler(Input(_): Input<Value>) -> StatusCode {
        StatusCode::NO_CONTENT
    }
    let app = Router::new().route("/", post(handler));
    for field in ["sql", "reason", "note", "name", "description", "tags"] {
        let body = serde_json::json!({field: "bad\u{0}"}).to_string();
        let response = app
            .clone()
            .oneshot(
                Request::post("/")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{field}");
    }
    let body = serde_json::json!({"tags":{"bad\u{1}":"value"}}).to_string();
    assert_eq!(
        app.oneshot(
            Request::post("/")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap()
        )
        .await
        .unwrap()
        .status(),
        StatusCode::BAD_REQUEST
    );
}

#[test]
fn cancellation_kind_does_not_depend_on_database_message() {
    use vda_server::error::ErrorKind;
    assert_eq!(
        ApiError::from(vda_connectors::ConnectorError::Cancelled).kind,
        ErrorKind::Cancelled
    );
    assert_eq!(
        ApiError::from(vda_connectors::ConnectorError::Database(
            "Query cancelled".into()
        ))
        .kind,
        ErrorKind::Database
    );
}

#[tokio::test]
async fn audit_shutdown_bounds_inflight_persistence_and_queued_events() {
    use vda_server::audit::Event;
    let db = sqlx::postgres::PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_secs(60))
        .connect_lazy("postgres://test@127.0.0.1:9/test")
        .unwrap();
    let (sender, receiver) = tokio::sync::mpsc::channel(4096);
    let stop = tokio_util::sync::CancellationToken::new();
    sender
        .send(Event {
            id: uuid::Uuid::new_v4(),
            actor_id: None,
            action: "test.event".into(),
            target_type: None,
            target_id: None,
            ip: None,
            details: serde_json::json!({}),
            created_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    let task = tokio::spawn(vda_server::audit::writer(
        db.clone(),
        receiver,
        stop.clone(),
    ));
    tokio::task::yield_now().await;
    stop.cancel();
    tokio::time::timeout(std::time::Duration::from_secs(6), task)
        .await
        .unwrap()
        .unwrap();
    db.close().await;
}
