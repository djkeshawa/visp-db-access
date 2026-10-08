#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Discovery API regressions using isolated schemas when VDA_TEST_DATABASE_URL is set.
use async_trait::async_trait;
use axum::{
    body::{to_bytes, Body},
    extract::ConnectInfo,
    http::{Request, StatusCode},
    Router,
};
use clap::Parser;
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::{Mutex, Notify};
use tower::ServiceExt;
use uuid::Uuid;
use vda_discovery::{DiscoveredDb, Provider, RegionError, ScanOutcome, SourceConfig, TestReport};
use vda_server::{
    app::{router, AppState},
    config::Cli,
    crypto::Crypto,
};

#[derive(Debug, Default)]
struct ControlledProvider {
    outcomes: Mutex<VecDeque<ScanOutcome>>,
    tested_config: Mutex<Option<SourceConfig>>,
    block_next: AtomicBool,
    block_test: AtomicBool,
    fail_test: AtomicBool,
    entered: Notify,
    release: Notify,
}
impl ControlledProvider {
    async fn enqueue(&self, resources: Vec<DiscoveredDb>, errors: Vec<RegionError>) {
        self.outcomes.lock().await.push_back(ScanOutcome {
            resources,
            errors,
            skipped: 0,
        });
    }
}
#[async_trait]
impl Provider for ControlledProvider {
    async fn test(&self, config: &SourceConfig) -> TestReport {
        *self.tested_config.lock().await = Some(config.clone());
        if self.block_test.swap(false, Ordering::SeqCst) {
            self.entered.notify_one();
            self.release.notified().await;
        }
        serde_json::from_value(json!({
            "ok":!self.fail_test.load(Ordering::SeqCst),"account_id":"123456789012",
            "identity_arn":"arn:aws:iam::123456789012:role/discovery",
            "regions":config.regions.iter().map(|r|json!({"region":r,"ok":true,"error":null})).collect::<Vec<_>>()
        })).unwrap()
    }
    async fn scan(&self, _: &SourceConfig) -> ScanOutcome {
        if self.block_next.swap(false, Ordering::SeqCst) {
            self.entered.notify_one();
            self.release.notified().await;
        }
        self.outcomes
            .lock()
            .await
            .pop_front()
            .unwrap_or(ScanOutcome {
                resources: vec![],
                errors: vec![],
                skipped: 0,
            })
    }
}

fn resource(identifier: &str, region: &str, host: &str) -> DiscoveredDb {
    serde_json::from_value(json!({
        "kind":"aurora_cluster",
        "arn":format!("arn:aws:rds:{region}:123456789012:cluster:{identifier}"),
        "identifier":identifier,"account_id":"123456789012","region":region,
        "engine":"postgres","engine_detail":"aurora-postgresql","engine_version":"16.4",
        "host":host,"port":55432,"replica_host":null,"replica_port":null,
        "database":"shop","status_detail":"available","publicly_accessible":false,
        "encrypted":true,"multi_az":true,"iam_auth_enabled":true,"vpc_id":"vpc-1234",
        "tags":{"environment":"development","owner":"platform"},
        "suggested_environment":"development"
    }))
    .unwrap()
}

struct Fixture {
    db: sqlx::PgPool,
    root: sqlx::PgPool,
    schema: String,
    state: AppState,
    writer: tokio::task::JoinHandle<()>,
    app: Router,
    provider: Arc<ControlledProvider>,
    project: Uuid,
    admin_cookie: String,
    member_cookie: String,
}
impl Fixture {
    async fn new() -> Option<Self> {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .with_test_writer()
            .try_init();
        let Ok(url) = std::env::var("VDA_TEST_DATABASE_URL") else {
            eprintln!("skipped: VDA_TEST_DATABASE_URL is not set");
            return None;
        };
        let root = sqlx::PgPool::connect(&url).await.unwrap();
        let schema = format!("vda_discovery_test_{}", Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&root)
            .await
            .unwrap();
        let search_path = format!("SET search_path TO {schema},public");
        let db = sqlx::postgres::PgPoolOptions::new()
            // Exercise transactions with a single metadata slot to catch nested pool acquisitions.
            .max_connections(1)
            .after_connect(move |conn, _| {
                let path = search_path.clone();
                Box::pin(async move {
                    sqlx::query(&path).execute(&mut *conn).await?;
                    sqlx::query("SET TIME ZONE 'UTC'").execute(conn).await?;
                    Ok(())
                })
            })
            .connect(&url)
            .await
            .unwrap();
        vda_server::db::migrate(&db).await.unwrap();
        let mut config = Cli::parse_from(["test"]).config;
        config.allow_private_targets = true;
        let (mut state, writer) = AppState::new(db.clone(), config, Crypto::new(&[8; 32]), None)
            .await
            .unwrap();
        let provider = Arc::new(ControlledProvider::default());
        state.discovery = provider.clone();
        let admin = Uuid::new_v4();
        let member = Uuid::new_v4();
        for (id, role) in [(admin, "admin"), (member, "member")] {
            sqlx::query(
                "INSERT INTO users(id,email,name,password_hash,org_role) VALUES ($1,$2,$2,$3,$4)",
            )
            .bind(id)
            .bind(format!("{role}@discovery.example"))
            .bind(&state.dummy_password_hash)
            .bind(role)
            .execute(&db)
            .await
            .unwrap();
        }
        let project = Uuid::new_v4();
        sqlx::query("INSERT INTO projects(id,name) VALUES ($1,'Discovery project')")
            .bind(project)
            .execute(&db)
            .await
            .unwrap();
        let admin_cookie = session(&db, admin).await;
        let member_cookie = session(&db, member).await;
        let app = router(state.clone());
        Some(Self {
            db,
            root,
            schema,
            state,
            writer,
            app,
            provider,
            project,
            admin_cookie,
            member_cookie,
        })
    }
    async fn request(&self, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        call(&self.app, method, path, &self.admin_cookie, body).await
    }
    async fn source(&self) -> Value {
        let (status, source) = self
            .request(
                "POST",
                "/discovery/sources",
                Some(json!({
                    "provider":"aws","name":"Discovery source","regions":["us-east-1","eu-west-1"],
                    "default_project_id":self.project
                })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{source}");
        source
    }
    async fn scan(&self, source: &Value) -> Value {
        let (status, run) = self
            .request(
                "POST",
                &format!("/discovery/sources/{}/scan", source["id"].as_str().unwrap()),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{run}");
        run
    }
    async fn resources(&self, source: &Value) -> Value {
        let (status, page) = self
            .request(
                "GET",
                &format!(
                    "/discovery/resources?source_id={}",
                    source["id"].as_str().unwrap()
                ),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{page}");
        page
    }
    fn import_body(&self) -> Value {
        json!({"project_id":self.project,"name":"Imported shop","environment":"development",
            "database":"shop","username":"shop_reader","password":"target-password","tls_mode":"disable"})
    }
    async fn close(self) {
        self.state.tasks.close();
        self.state.tasks.wait().await;
        self.state.stop.cancel();
        self.writer.await.unwrap();
        self.state.pools.close().await;
        self.db.close().await;
        sqlx::query(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .execute(&self.root)
            .await
            .unwrap();
        self.root.close().await;
    }
}
async fn session(db: &sqlx::PgPool, user: Uuid) -> String {
    use base64::Engine;
    let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
        Uuid::new_v4()
            .as_bytes()
            .iter()
            .copied()
            .chain(Uuid::new_v4().as_bytes().iter().copied())
            .collect::<Vec<_>>(),
    );
    sqlx::query("INSERT INTO sessions(token_hash,user_id,expires_at) VALUES ($1,$2,now()+interval '1 hour')")
        .bind(vda_server::auth::token_hash(&token)).bind(user).execute(db).await.unwrap();
    format!("vda_session={token}")
}
async fn call(
    app: &Router,
    method: &str,
    path: &str,
    cookie: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(format!("/api/v1{path}"))
        .header("cookie", cookie)
        .header("x-requested-with", "vda");
    let body = if let Some(body) = body {
        req = req.header("content-type", "application/json");
        Body::from(body.to_string())
    } else {
        Body::empty()
    };
    let mut req = req.body(body).unwrap();
    req.extensions_mut().insert(ConnectInfo(
        "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
    ));
    let response = app.clone().oneshot(req).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, body)
}
fn identified<'a>(page: &'a Value, identifier: &str) -> &'a Value {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["identifier"] == identifier)
        .unwrap()
}
fn assert_write_only(value: &Value, secret: &str) {
    let encoded = value.to_string();
    assert!(!encoded.contains(secret), "{encoded}");
    assert!(value.get("external_id").is_none());
    assert!(value.get("external_id_enc").is_none());
}

#[tokio::test]
async fn source_crud_validates_inputs_and_external_id_is_encrypted_and_write_only() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let mut body = json!({"provider":"aws","name":"Cross account","regions":["us-east-1"],
        "role_arn":"arn:aws:iam::123456789012:role/platform/discovery","external_id":"tenant-secret-unique"});
    let (status, source) = f
        .request("POST", "/discovery/sources", Some(body.clone()))
        .await;
    assert_eq!(status, StatusCode::OK, "{source}");
    assert_eq!(source["external_id_set"], true);
    assert_eq!(source["scan_interval_minutes"], 60);
    assert_eq!(
        source["environment_tag_keys"],
        json!(["environment", "env", "stage"])
    );
    assert_write_only(&source, "tenant-secret-unique");
    let id = Uuid::parse_str(source["id"].as_str().unwrap()).unwrap();
    let ciphertext: String =
        sqlx::query_scalar("SELECT external_id_enc FROM discovery_sources WHERE id=$1")
            .bind(id)
            .fetch_one(&f.db)
            .await
            .unwrap();
    assert_ne!(ciphertext, "tenant-secret-unique");
    assert_eq!(
        f.state.crypto.decrypt(id, &ciphertext).unwrap(),
        "tenant-secret-unique"
    );
    assert!(f.state.crypto.decrypt(Uuid::new_v4(), &ciphertext).is_err());
    let (_, listed) = f.request("GET", "/discovery/sources", None).await;
    assert_write_only(&listed, "tenant-secret-unique");
    let path = format!("/discovery/sources/{id}");
    let (status, preserved) = f
        .request("PATCH", &path, Some(json!({"name":"Renamed"})))
        .await;
    assert_eq!(status, StatusCode::OK, "{preserved}");
    assert_eq!(preserved["external_id_set"], true);
    let preserved_ciphertext: String =
        sqlx::query_scalar("SELECT external_id_enc FROM discovery_sources WHERE id=$1")
            .bind(id)
            .fetch_one(&f.db)
            .await
            .unwrap();
    assert_eq!(preserved_ciphertext, ciphertext);
    let (status, patched) = f.request("PATCH",&path,Some(json!({"name":"Updated","external_id":null,"enabled":false,"scan_interval_minutes":5}))).await;
    assert_eq!(status, StatusCode::OK, "{patched}");
    assert_eq!(patched["external_id_set"], false);
    assert_eq!(patched["name"], "Updated");
    let cleared: Option<String> =
        sqlx::query_scalar("SELECT external_id_enc FROM discovery_sources WHERE id=$1")
            .bind(id)
            .fetch_one(&f.db)
            .await
            .unwrap();
    assert!(cleared.is_none());
    for (field, invalid) in [
        ("provider", json!("gcp")),
        ("name", json!("")),
        ("name", json!("bad\u{0000}name")),
        ("regions", json!([])),
        ("regions", json!(["not-a-region"])),
        ("regions", json!(vec!["us-east-1"; 21])),
        ("role_arn", json!("arn:aws:iam::123:role/discovery")),
        (
            "role_arn",
            json!("arn:aws:iam::123456789012:user/discovery"),
        ),
        ("role_arn", json!("arn:aws:iam::123456789012:role/équipe")),
        (
            "role_arn",
            json!(format!(
                "arn:aws:iam::123456789012:role/{}",
                "a".repeat(513)
            )),
        ),
        ("scan_interval_minutes", json!(4)),
        ("scan_interval_minutes", json!(1441)),
        ("environment_tag_keys", json!(["bad\u{0001}key"])),
    ] {
        let mut invalid_body = body.clone();
        invalid_body[field] = invalid;
        let (status, error) = f
            .request("POST", "/discovery/sources", Some(invalid_body))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "field={field}: {error}");
        assert_eq!(error["error"]["code"], "validation");
    }
    for role in [
        "arn:aws-cn:iam::123456789012:role/discovery".to_owned(),
        "arn:aws-us-gov:iam::123456789012:role/discovery".to_owned(),
        format!("arn:aws:iam::123456789012:role/{}", "a".repeat(512)),
    ] {
        let mut valid = body.clone();
        valid["role_arn"] = json!(role);
        let (status, created) = f.request("POST", "/discovery/sources", Some(valid)).await;
        assert_eq!(status, StatusCode::OK, "{created}");
        let (status, _) = f
            .request(
                "DELETE",
                &format!("/discovery/sources/{}", created["id"].as_str().unwrap()),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }
    body["external_id"] = json!("replacement-secret");
    let (status, replaced) = f
        .request(
            "PATCH",
            &path,
            Some(json!({"external_id":body["external_id"]})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{replaced}");
    assert_eq!(replaced["external_id_set"], true);
    assert_write_only(&replaced, "replacement-secret");
    let (status, report) = f.request("POST", &format!("{path}/test"), None).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["ok"], true);
    assert_eq!(report["regions"][0]["region"], "us-east-1");
    {
        use secrecy::ExposeSecret;
        let tested = f.provider.tested_config.lock().await;
        let tested = tested.as_ref().unwrap();
        assert_eq!(
            tested.role_arn.as_deref(),
            Some("arn:aws:iam::123456789012:role/platform/discovery")
        );
        assert_eq!(
            tested.external_id.as_ref().unwrap().expose_secret(),
            "replacement-secret"
        );
    }
    let (status, _) = f.request("DELETE", &path, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, listed) = f.request("GET", "/discovery/sources", None).await;
    assert!(listed["items"].as_array().unwrap().is_empty());
    f.close().await;
}

#[tokio::test]
async fn scans_upsert_preserve_ignored_and_only_mark_successful_regions_gone() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let source = f.source().await;
    let alpha = resource("alpha-dev", "us-east-1", "127.0.0.1");
    let beta = resource("beta-prod", "eu-west-1", "127.0.0.2");
    let gamma = resource("gamma-dev", "us-east-1", "127.0.0.3");
    f.provider
        .enqueue(vec![alpha.clone(), beta.clone(), gamma.clone()], vec![])
        .await;
    let first = f.scan(&source).await;
    assert_eq!(first["status"], "succeeded");
    assert_eq!(first["found"], 3);
    assert_eq!(first["new"], 3);
    let initial = f.resources(&source).await;
    let source_id = source["id"].as_str().unwrap();
    let (status, first_page) = f
        .request(
            "GET",
            &format!("/discovery/resources?source_id={source_id}&limit=1"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{first_page}");
    assert_eq!(first_page["items"].as_array().unwrap().len(), 1);
    let (status, second_page) = f
        .request(
            "GET",
            &format!(
                "/discovery/resources?source_id={source_id}&limit=1&cursor={}",
                first_page["next_cursor"].as_str().unwrap()
            ),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{second_page}");
    assert_ne!(first_page["items"][0]["id"], second_page["items"][0]["id"]);
    let (status, matched) = f.request("GET", &format!("/discovery/resources?source_id={source_id}&engine=postgres&region=us-east-1&q=ALPHA"), None).await;
    assert_eq!(status, StatusCode::OK, "{matched}");
    assert_eq!(matched["items"].as_array().unwrap().len(), 1);
    assert_eq!(matched["items"][0]["identifier"], "alpha-dev");
    for query in [
        "limit=0",
        "limit=201",
        "cursor=invalid",
        "status=invalid",
        "engine=invalid",
        "q=%00",
    ] {
        let (status, error) = f
            .request("GET", &format!("/discovery/resources?{query}"), None)
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {error}");
    }
    let alpha_id = identified(&initial, "alpha-dev")["id"].clone();
    let gamma_id = identified(&initial, "gamma-dev")["id"].as_str().unwrap();
    let (status, ignored) = f
        .request(
            "POST",
            &format!("/discovery/resources/{gamma_id}/ignore"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{ignored}");
    assert_eq!(ignored["status"], "ignored");
    let (status, unignored) = f
        .request(
            "POST",
            &format!("/discovery/resources/{gamma_id}/unignore"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{unignored}");
    assert_eq!(unignored["status"], "new");
    let (status, ignored) = f
        .request(
            "POST",
            &format!("/discovery/resources/{gamma_id}/ignore"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{ignored}");
    f.provider
        .enqueue(
            vec![alpha.clone(), gamma],
            vec![RegionError {
                region: Some("eu-west-1".into()),
                message: "Access denied".into(),
            }],
        )
        .await;
    let partial = f.scan(&source).await;
    assert_eq!(partial["status"], "partial");
    assert_eq!(partial["new"], 0);
    assert_eq!(partial["gone"], 0);
    let partial_page = f.resources(&source).await;
    assert_eq!(identified(&partial_page, "beta-prod")["status"], "new");
    assert_eq!(identified(&partial_page, "gamma-dev")["status"], "ignored");
    assert_eq!(identified(&partial_page, "alpha-dev")["id"], alpha_id);
    f.provider.enqueue(vec![alpha.clone()], vec![]).await;
    let absent = f.scan(&source).await;
    assert_eq!(absent["gone"], 2);
    let absent_page = f.resources(&source).await;
    assert_eq!(identified(&absent_page, "beta-prod")["status"], "gone");
    assert_eq!(identified(&absent_page, "gamma-dev")["status"], "gone");
    assert_eq!(absent_page["counts"]["gone"], 2);
    let source_id = source["id"].as_str().unwrap();
    let (_, filtered) = f
        .request(
            "GET",
            &format!("/discovery/resources?source_id={source_id}&status=gone"),
            None,
        )
        .await;
    assert_eq!(filtered["items"].as_array().unwrap().len(), 2);
    assert_eq!(filtered["counts"]["new"], 1);
    let (_, runs) = f
        .request(
            "GET",
            &format!("/discovery/sources/{source_id}/runs?limit=1"),
            None,
        )
        .await;
    assert_eq!(runs["items"].as_array().unwrap().len(), 1);
    assert!(runs["next_cursor"].is_string());
    let (_, next) = f
        .request(
            "GET",
            &format!(
                "/discovery/sources/{source_id}/runs?limit=1&cursor={}",
                runs["next_cursor"].as_str().unwrap()
            ),
            None,
        )
        .await;
    assert_ne!(next["items"][0]["id"], runs["items"][0]["id"]);
    f.close().await;
}

#[tokio::test]
async fn import_creates_encrypted_cluster_and_drift_sync_requires_password() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let source = f.source().await;
    let mut original = resource("shop-dev", "us-east-1", "127.0.0.1");
    original.replica_host = Some("127.0.0.4".into());
    original.replica_port = Some(55432);
    f.provider.enqueue(vec![original.clone()], vec![]).await;
    f.scan(&source).await;
    let page = f.resources(&source).await;
    let discovered = identified(&page, "shop-dev");
    let resource_path = format!(
        "/discovery/resources/{}",
        discovered["id"].as_str().unwrap()
    );
    let body = f.import_body();
    let (status, cluster) = f
        .request(
            "POST",
            &format!("{resource_path}/import"),
            Some(body.clone()),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{cluster}");
    for (field, expected) in [
        ("project_id", json!(f.project)),
        ("host", json!("127.0.0.1")),
        ("port", json!(55432)),
        ("engine", json!("postgres")),
        ("provider", json!("aws")),
        ("region", json!("us-east-1")),
        ("environment", json!("development")),
        ("database", json!("shop")),
        ("username", json!("shop_reader")),
        ("replica_host", json!("127.0.0.4")),
        ("replica_port", json!(55432)),
    ] {
        assert_eq!(cluster[field], expected, "{field}: {cluster}");
    }
    assert!(cluster.get("password").is_none());
    assert!(cluster.get("password_enc").is_none());
    assert_eq!(cluster["tags"]["aws:arn"], discovered["arn"]);
    assert_eq!(cluster["tags"]["aws:account"], "123456789012");
    let id = Uuid::parse_str(cluster["id"].as_str().unwrap()).unwrap();
    let ciphertext: String = sqlx::query_scalar("SELECT password_enc FROM clusters WHERE id=$1")
        .bind(id)
        .fetch_one(&f.db)
        .await
        .unwrap();
    assert_eq!(
        f.state.crypto.decrypt(id, &ciphertext).unwrap(),
        "target-password"
    );
    let policy = vda_server::db::policy(&f.db, id).await.unwrap();
    assert_eq!(policy.max_rows, 5000);
    let (status, error) = f
        .request("POST", &format!("{resource_path}/import"), Some(body))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{error}");
    assert_eq!(error["error"]["code"], "conflict");
    let mut changed = original.clone();
    changed.host = "127.0.0.2".into();
    changed.replica_host = Some("127.0.0.3".into());
    changed.replica_port = Some(55432);
    f.provider.enqueue(vec![changed.clone()], vec![]).await;
    let run = f.scan(&source).await;
    assert_eq!(run["changed"], 1);
    let page = f.resources(&source).await;
    let drifted = identified(&page, "shop-dev");
    assert_eq!(drifted["status"], "imported");
    assert_eq!(drifted["cluster_id"], cluster["id"]);
    assert!(drifted["drift"]
        .as_array()
        .unwrap()
        .contains(&json!("endpoint_changed")));
    assert!(drifted["drift"]
        .as_array()
        .unwrap()
        .contains(&json!("replica_changed")));
    let (_, overview) = f.request("GET", "/overview", None).await;
    assert_eq!(overview["discovery"]["drifted"], 1);
    let (status, error) = f
        .request("POST", &format!("{resource_path}/sync"), Some(json!({})))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    assert_eq!(
        error["error"]["message"],
        "Changing the connection endpoint requires re-entering the password"
    );
    let stored = vda_server::db::cluster(&f.db, id).await.unwrap();
    assert_eq!(stored.host, "127.0.0.1");
    let (status, synced) = f
        .request(
            "POST",
            &format!("{resource_path}/sync"),
            Some(json!({"password":"new-target-password"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{synced}");
    assert_eq!(synced["host"], "127.0.0.2");
    assert_eq!(synced["replica_host"], "127.0.0.3");
    let stored = vda_server::db::cluster(&f.db, id).await.unwrap();
    assert_eq!(
        f.state.crypto.decrypt(id, &stored.password_enc).unwrap(),
        "new-target-password"
    );
    let page = f.resources(&source).await;
    assert_eq!(identified(&page, "shop-dev")["drift"], json!([]));
    f.provider.enqueue(vec![changed.clone()], vec![]).await;
    f.scan(&source).await;
    let page = f.resources(&source).await;
    assert_eq!(identified(&page, "shop-dev")["status"], "imported");
    assert_eq!(identified(&page, "shop-dev")["drift"], json!([]));
    changed.engine = "mysql".into();
    changed.engine_detail = "aurora-mysql".into();
    f.provider.enqueue(vec![changed], vec![]).await;
    f.scan(&source).await;
    let page = f.resources(&source).await;
    assert!(identified(&page, "shop-dev")["drift"]
        .as_array()
        .unwrap()
        .contains(&json!("engine_changed")));
    let (status, unchanged) = f
        .request("POST", &format!("{resource_path}/sync"), Some(json!({})))
        .await;
    assert_eq!(status, StatusCode::OK, "{unchanged}");
    assert_eq!(unchanged["engine"], "postgres");
    f.provider.enqueue(vec![], vec![]).await;
    f.scan(&source).await;
    let page = f.resources(&source).await;
    let gone = identified(&page, "shop-dev");
    assert_eq!(gone["status"], "gone");
    assert!(gone["drift"]
        .as_array()
        .unwrap()
        .contains(&json!("deleted")));
    let (status, _) = f
        .request(
            "DELETE",
            &format!("/discovery/sources/{}", source["id"].as_str().unwrap()),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(f.resources(&source).await["items"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(vda_server::db::cluster(&f.db, id).await.is_ok());
    f.close().await;
}

#[tokio::test]
async fn discovery_requires_org_admin_and_obeys_network_gate() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let source = f.source().await;
    f.provider
        .enqueue(
            vec![resource("admin-dev", "us-east-1", "127.0.0.1")],
            vec![],
        )
        .await;
    f.scan(&source).await;
    let page = f.resources(&source).await;
    let resource_id = page["items"][0]["id"].as_str().unwrap();
    let source_id = source["id"].as_str().unwrap();
    for (method, path, body) in [
        ("GET", "/discovery/sources".into(), None),
        (
            "POST",
            "/discovery/sources".into(),
            Some(json!({"provider":"aws","name":"Forbidden","regions":["us-east-1"]})),
        ),
        (
            "PATCH",
            format!("/discovery/sources/{source_id}"),
            Some(json!({"name":"Forbidden"})),
        ),
        ("DELETE", format!("/discovery/sources/{source_id}"), None),
        ("POST", format!("/discovery/sources/{source_id}/test"), None),
        ("POST", format!("/discovery/sources/{source_id}/scan"), None),
        ("GET", format!("/discovery/sources/{source_id}/runs"), None),
        ("GET", "/discovery/resources".into(), None),
        (
            "POST",
            format!("/discovery/resources/{resource_id}/import"),
            Some(f.import_body()),
        ),
        (
            "POST",
            format!("/discovery/resources/{resource_id}/ignore"),
            None,
        ),
        (
            "POST",
            format!("/discovery/resources/{resource_id}/unignore"),
            None,
        ),
        (
            "POST",
            format!("/discovery/resources/{resource_id}/sync"),
            Some(json!({})),
        ),
    ] {
        let (status, error) = call(&f.app, method, &path, &f.member_cookie, body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}: {error}");
        assert_eq!(error["error"]["code"], "forbidden");
    }
    let (_, overview) = call(&f.app, "GET", "/overview", &f.member_cookie, None).await;
    assert!(overview["discovery"].is_null());
    sqlx::query("UPDATE settings SET value=jsonb_set(value,'{allowed_cidrs}','[\"203.0.113.0/24\"]') WHERE key='network'")
        .execute(&f.db).await.unwrap();
    f.state.invalidate_network().await;
    let (status, error) = f.request("GET", "/discovery/sources", None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{error}");
    f.close().await;
}

#[tokio::test]
async fn concurrent_scan_returns_conflict_without_starting_second_run() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let source = f.source().await;
    f.provider
        .enqueue(
            vec![resource("concurrent-dev", "us-east-1", "127.0.0.1")],
            vec![],
        )
        .await;
    f.provider.block_next.store(true, Ordering::SeqCst);
    let app = f.app.clone();
    let cookie = f.admin_cookie.clone();
    let path = format!("/discovery/sources/{}/scan", source["id"].as_str().unwrap());
    let running_path = path.clone();
    let running =
        tokio::spawn(async move { call(&app, "POST", &running_path, &cookie, None).await });
    tokio::time::timeout(Duration::from_secs(5), f.provider.entered.notified())
        .await
        .unwrap();
    let (status, error) = f.request("POST", &path, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{error}");
    assert_eq!(error["error"]["code"], "conflict");
    f.provider.release.notify_one();
    let (status, run) = tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status, StatusCode::OK, "{run}");
    let (_, runs) = f
        .request(
            "GET",
            &format!("/discovery/sources/{}/runs", source["id"].as_str().unwrap()),
            None,
        )
        .await;
    assert_eq!(runs["items"].as_array().unwrap().len(), 1);
    f.close().await;
}

#[tokio::test]
async fn failed_scan_preserves_inventory_and_recovers_stale_running_claim() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let source = f.source().await;
    f.provider
        .enqueue(
            vec![resource("retained-prod", "us-east-1", "127.0.0.1")],
            vec![],
        )
        .await;
    f.scan(&source).await;
    f.provider
        .enqueue(
            vec![],
            vec![RegionError {
                region: None,
                message: "Identity unavailable".into(),
            }],
        )
        .await;
    let failed = f.scan(&source).await;
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["gone"], 0);
    assert_eq!(f.resources(&source).await["items"][0]["status"], "new");
    let stale = Uuid::new_v4();
    let source_id = Uuid::parse_str(source["id"].as_str().unwrap()).unwrap();
    sqlx::query("INSERT INTO discovery_runs(id,source_id,status,started_at) VALUES($1,$2,'running',now()-interval '11 minutes')")
        .bind(stale).bind(source_id).execute(&f.db).await.unwrap();
    sqlx::query("UPDATE discovery_sources SET status='running' WHERE id=$1")
        .bind(source_id)
        .execute(&f.db)
        .await
        .unwrap();
    f.provider
        .enqueue(
            vec![resource("retained-prod", "us-east-1", "127.0.0.1")],
            vec![],
        )
        .await;
    let resumed = f.scan(&source).await;
    assert_eq!(resumed["status"], "succeeded");
    let (status, finished): (String, bool) =
        sqlx::query_as("SELECT status,finished_at IS NOT NULL FROM discovery_runs WHERE id=$1")
            .bind(stale)
            .fetch_one(&f.db)
            .await
            .unwrap();
    assert_eq!(status, "failed");
    assert!(finished);
    f.close().await;
}

#[tokio::test]
async fn import_and_sync_reject_cloud_metadata_endpoints() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let source = f.source().await;
    f.provider
        .enqueue(
            vec![resource("unsafe-prod", "us-east-1", "169.254.169.254")],
            vec![],
        )
        .await;
    f.scan(&source).await;
    let page = f.resources(&source).await;
    let path = format!(
        "/discovery/resources/{}",
        page["items"][0]["id"].as_str().unwrap()
    );
    let (status, error) = f
        .request("POST", &format!("{path}/import"), Some(f.import_body()))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM clusters")
        .fetch_one(&f.db)
        .await
        .unwrap();
    assert_eq!(count, 0);
    f.provider
        .enqueue(
            vec![resource("unsafe-prod", "us-east-1", "127.0.0.1")],
            vec![],
        )
        .await;
    f.scan(&source).await;
    let (status, cluster) = f
        .request("POST", &format!("{path}/import"), Some(f.import_body()))
        .await;
    assert_eq!(status, StatusCode::OK, "{cluster}");
    let cluster_id = Uuid::parse_str(cluster["id"].as_str().unwrap()).unwrap();
    f.provider
        .enqueue(
            vec![resource("unsafe-prod", "us-east-1", "169.254.169.254")],
            vec![],
        )
        .await;
    f.scan(&source).await;
    let (status, error) = f
        .request(
            "POST",
            &format!("{path}/sync"),
            Some(json!({"password":"new-password"})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    assert_eq!(
        vda_server::db::cluster(&f.db, cluster_id)
            .await
            .unwrap()
            .host,
        "127.0.0.1"
    );
    f.close().await;
}

#[tokio::test]
async fn draft_tests_validate_without_saving_and_persist_on_save() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let draft = json!({"provider":"aws","name":"Draft","regions":["us-east-1"],"external_id":"draft-secret"});
    let (status, report) = f
        .request("POST", "/discovery/sources/test", Some(draft.clone()))
        .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["ok"], true);
    let (_, sources) = f.request("GET", "/discovery/sources", None).await;
    assert_eq!(sources["items"], json!([]));
    let (status, source) = f.request("POST", "/discovery/sources", Some(draft)).await;
    assert_eq!(status, StatusCode::OK, "{source}");
    assert_eq!(source["last_test"]["ok"], true);
    assert_eq!(source["last_test"]["account_id"], "123456789012");
    assert!(source["last_test"]["tested_at"].is_string());
    assert_write_only(&source, "draft-secret");
    let path = format!("/discovery/sources/{}", source["id"].as_str().unwrap());
    let (_, renamed) = f
        .request("PATCH", &path, Some(json!({"name":"Renamed"})))
        .await;
    assert_eq!(renamed["last_test"], source["last_test"]);
    let (_, changed) = f
        .request("PATCH", &path, Some(json!({"regions":["eu-west-1"]})))
        .await;
    assert!(changed["last_test"].is_null());
    let (status, _) = f.request("POST", &format!("{path}/test"), None).await;
    assert_eq!(status, StatusCode::OK);
    let (_, listed) = f.request("GET", "/discovery/sources", None).await;
    assert_eq!(listed["items"][0]["last_test"]["ok"], true);
    let persisted: Value =
        sqlx::query_scalar("SELECT last_test FROM discovery_sources WHERE id=$1")
            .bind(Uuid::parse_str(source["id"].as_str().unwrap()).unwrap())
            .fetch_one(&f.db)
            .await
            .unwrap();
    assert_eq!(persisted, listed["items"][0]["last_test"]);
    for body in [
        json!({"provider":"aws","name":"bad","regions":[]}),
        json!({"provider":"aws","name":"bad","regions":["us-east-1"],"last_test":{"ok":true}}),
    ] {
        assert_eq!(
            f.request("POST", "/discovery/sources/test", Some(body))
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        call(
            &f.app,
            "POST",
            "/discovery/sources/test",
            &f.member_cookie,
            Some(json!({"provider":"aws","name":"Member","regions":["us-east-1"]}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    f.close().await;
}

#[tokio::test]
async fn test_results_do_not_verify_changed_configs_and_failures_are_persisted() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let source = f.source().await;
    let path = format!("/discovery/sources/{}", source["id"].as_str().unwrap());
    f.provider.block_test.store(true, Ordering::SeqCst);
    let app = f.app.clone();
    let cookie = f.admin_cookie.clone();
    let test_path = format!("{path}/test");
    let pending = tokio::spawn(async move { call(&app, "POST", &test_path, &cookie, None).await });
    f.provider.entered.notified().await;
    assert_eq!(
        f.request("PATCH", &path, Some(json!({"regions":["ap-southeast-2"]})))
            .await
            .0,
        StatusCode::OK
    );
    f.provider.release.notify_one();
    assert_eq!(pending.await.unwrap().0, StatusCode::OK);
    let (_, listed) = f.request("GET", "/discovery/sources", None).await;
    assert!(listed["items"][0]["last_test"].is_null());
    f.provider.fail_test.store(true, Ordering::SeqCst);
    let (_, report) = f.request("POST", &format!("{path}/test"), None).await;
    assert_eq!(report["ok"], false);
    let (_, listed) = f.request("GET", "/discovery/sources", None).await;
    assert_eq!(listed["items"][0]["last_test"]["ok"], false);
    f.close().await;
}
