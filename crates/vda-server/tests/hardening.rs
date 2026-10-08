#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Security regressions against isolated metadata schemas and an optional live target.
use axum::{
    body::{to_bytes, Body},
    extract::ConnectInfo,
    http::{Request, StatusCode},
    Router,
};
use clap::Parser;
use serde_json::{json, Value};
use std::net::SocketAddr;
use tower::ServiceExt;
use uuid::Uuid;
use vda_server::{
    app::{router, AppState},
    config::Cli,
    crypto::Crypto,
    policy::Policy,
};

struct Fixture {
    db: sqlx::PgPool,
    root: sqlx::PgPool,
    schema: String,
    state: AppState,
    writer: tokio::task::JoinHandle<()>,
    app: Router,
    admin: Uuid,
    grantor: Uuid,
    recipient: Uuid,
    cluster: Uuid,
    project: Uuid,
    admin_cookie: String,
    grantor_cookie: String,
}
impl Fixture {
    async fn new() -> Option<Self> {
        let Ok(url) = std::env::var("VDA_TEST_DATABASE_URL") else {
            eprintln!("skipped: VDA_TEST_DATABASE_URL is not set");
            return None;
        };
        let root = sqlx::PgPool::connect(&url).await.unwrap();
        let schema = format!("vda_harden_{}", Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&root)
            .await
            .unwrap();
        let path = format!("SET search_path TO {schema},public");
        let db = sqlx::postgres::PgPoolOptions::new()
            .max_connections(10)
            .after_connect(move |c, _| {
                let path = path.clone();
                Box::pin(async move {
                    sqlx::query(&path).execute(&mut *c).await?;
                    sqlx::query("SET TIME ZONE 'UTC'").execute(c).await?;
                    Ok(())
                })
            })
            .connect(&url)
            .await
            .unwrap();
        vda_server::db::migrate(&db).await.unwrap();
        let mut config = Cli::parse_from(["test"]).config;
        config.allow_private_targets = true;
        config.metrics_token = Some("metrics-secret".into());
        let crypto = Crypto::new(&[4; 32]);
        let (state, writer) = AppState::new(db.clone(), config, crypto.clone(), None)
            .await
            .unwrap();
        let (admin, grantor, recipient, project, cluster) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        for (id, email, role) in [
            (admin, "admin@harden.example", "admin"),
            (grantor, "grantor@harden.example", "member"),
            (recipient, "recipient@harden.example", "member"),
        ] {
            sqlx::query(
                "INSERT INTO users(id,email,name,password_hash,org_role) VALUES ($1,$2,$2,$3,$4)",
            )
            .bind(id)
            .bind(email)
            .bind(&state.dummy_password_hash)
            .bind(role)
            .execute(&db)
            .await
            .unwrap();
        }
        sqlx::query("INSERT INTO projects(id,name) VALUES ($1,'Project')")
            .bind(project)
            .execute(&db)
            .await
            .unwrap();
        let target_url = std::env::var("VDA_TEST_POSTGRES_URL")
            .unwrap_or_else(|_| "postgres://vda@127.0.0.1:9/postgres".into());
        let target = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy(&target_url)
            .unwrap();
        let opts = target.connect_options();
        let password = target_url
            .split_once("://")
            .and_then(|(_, s)| s.split_once('@'))
            .and_then(|(s, _)| s.split_once(':'))
            .map(|(_, p)| p)
            .unwrap_or("");
        sqlx::query("INSERT INTO clusters(id,project_id,name,engine,provider,region,environment,host,port,database,username,password_enc,tls_mode)
            VALUES ($1,$2,'Cluster','postgres','onprem','local','development',$3,$4,$5,$6,$7,'disable')")
            .bind(cluster).bind(project).bind(opts.get_host()).bind(i32::from(opts.get_port()))
            .bind(opts.get_database().unwrap_or("postgres")).bind(opts.get_username())
            .bind(crypto.encrypt(cluster,password).unwrap()).execute(&db).await.unwrap();
        target.close().await;
        let mut policy = Policy::for_environment("development");
        policy.require_approval_for_writes = false;
        sqlx::query("INSERT INTO cluster_policies(cluster_id,policy) VALUES ($1,$2)")
            .bind(cluster)
            .bind(json!(policy))
            .execute(&db)
            .await
            .unwrap();
        sqlx::query("INSERT INTO grants(id,user_id,scope,scope_id,level,expires_at) VALUES ($1,$2,'cluster',$3,'admin',now()+interval '1 hour')")
            .bind(Uuid::new_v4()).bind(grantor).bind(cluster).execute(&db).await.unwrap();
        let admin_cookie = Self::session(&db, admin).await;
        let grantor_cookie = Self::session(&db, grantor).await;
        let app = router(state.clone());
        Some(Self {
            db,
            root,
            schema,
            state,
            writer,
            app,
            admin,
            grantor,
            recipient,
            cluster,
            project,
            admin_cookie,
            grantor_cookie,
        })
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
    async fn request(
        &self,
        method: &str,
        path: &str,
        cookie: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        call(
            &self.app,
            method,
            &format!("/api/v1{path}"),
            cookie,
            body,
            None,
            "127.0.0.1:1234",
        )
        .await
    }
    async fn policy(&self, policy: &Policy) {
        let (s, v) = self
            .request(
                "PUT",
                &format!("/clusters/{}/policy", self.cluster),
                &self.admin_cookie,
                Some(json!(policy)),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
    }
    async fn approval(&self, sql: &str, status: &str) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO approvals(id,cluster_id,requester_id,sql,reason,analysis,status,reviewer_id,reviewed_at)
            VALUES ($1,$2,$3,$4,'Test','{}',$5,$6,now())")
            .bind(id).bind(self.cluster).bind(self.grantor).bind(sql).bind(status)
            .bind(if status=="pending" {None} else {Some(self.admin)}).execute(&self.db).await.unwrap();
        id
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
async fn call(
    app: &Router,
    method: &str,
    path: &str,
    cookie: &str,
    body: Option<Value>,
    bearer: Option<&str>,
    peer: &str,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("cookie", cookie)
        .header("x-requested-with", "vda");
    if let Some(bearer) = bearer {
        req = req.header("authorization", bearer);
    }
    let body = match body {
        Some(v) => {
            req = req.header("content-type", "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    let mut req = req.body(body).unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(peer.parse::<SocketAddr>().unwrap()));
    let response = app.clone().oneshot(req).await.unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 40 * 1024 * 1024)
        .await
        .unwrap();
    let value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    (status, value)
}

#[tokio::test]
async fn endpoint_changes_and_delegation_cannot_reuse_or_extend_authority() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let path = format!("/clusters/{}", f.cluster);
    let stored = vda_server::db::cluster(&f.db, f.cluster).await.unwrap();
    for patch in [
        json!({"host":"localhost"}),
        json!({"port":1234}),
        json!({"database":"other"}),
        json!({"username":"other"}),
        json!({"engine":"mysql"}),
        json!({"tls_mode":"prefer"}),
        json!({"replica_host":"localhost"}),
        json!({"replica_port":1234}),
    ] {
        for cookie in [&f.grantor_cookie, &f.admin_cookie] {
            let (s, v) = f.request("PATCH", &path, cookie, Some(patch.clone())).await;
            assert_eq!(s, StatusCode::BAD_REQUEST, "{patch}: {v}");
            assert_eq!(
                v["error"]["message"],
                "Changing the connection endpoint requires re-entering the password"
            );
        }
    }
    let (s, v) = f
        .request(
            "PATCH",
            &path,
            &f.grantor_cookie,
            Some(json!({"host":stored.host,"name":"Renamed"})),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v) = f
        .request(
            "PATCH",
            &path,
            &f.grantor_cookie,
            Some(json!({"host":"localhost","password":"fresh-password"})),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let fresh = vda_server::db::cluster(&f.db, f.cluster).await.unwrap();
    assert_eq!(
        f.state
            .crypto
            .decrypt(f.cluster, &fresh.password_enc)
            .unwrap(),
        "fresh-password"
    );
    let (s,v)=f.request("POST","/clusters/test-connection",&f.admin_cookie,Some(json!({"cluster_id":f.cluster,"engine":"postgres","host":"127.0.0.1","port":fresh.port,"database":fresh.database,"username":fresh.username,"tls_mode":"disable"}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let grant =
        json!({"user_id":f.recipient,"scope":"cluster","scope_id":f.cluster,"level":"read"});
    for expiry in [
        Value::Null,
        json!(chrono::Utc::now() + chrono::Duration::hours(2)),
    ] {
        let mut body = grant.clone();
        body["expires_at"] = expiry;
        assert_eq!(
            f.request("POST", "/grants", &f.grantor_cookie, Some(body))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    let mut self_grant = grant.clone();
    self_grant["user_id"] = json!(f.grantor);
    self_grant["expires_at"] = json!(chrono::Utc::now() + chrono::Duration::minutes(30));
    assert_eq!(
        f.request("POST", "/grants", &f.grantor_cookie, Some(self_grant))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let mut valid = grant.clone();
    valid["expires_at"] = json!(chrono::Utc::now() + chrono::Duration::minutes(30));
    valid["level"] = json!("admin");
    assert_eq!(
        f.request("POST", "/grants", &f.grantor_cookie, Some(valid.clone()))
            .await
            .0,
        StatusCode::OK
    );
    valid["scope"] = json!("project");
    valid["scope_id"] = json!(f.project);
    assert_eq!(
        f.request("POST", "/grants", &f.grantor_cookie, Some(valid.clone()))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        f.request("POST", "/grants", &f.admin_cookie, Some(grant))
            .await
            .0,
        StatusCode::OK
    );
    sqlx::query("INSERT INTO grants(id,user_id,scope,scope_id,level,expires_at) VALUES ($1,$2,'project',$3,'admin',now()+interval '20 minutes')")
        .bind(Uuid::new_v4()).bind(f.grantor).bind(f.project).execute(&f.db).await.unwrap();
    assert_eq!(
        f.request("POST", "/grants", &f.grantor_cookie, Some(valid.clone()))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    valid["expires_at"] = json!(chrono::Utc::now() + chrono::Duration::minutes(10));
    assert_eq!(
        f.request("POST", "/grants", &f.grantor_cookie, Some(valid))
            .await
            .0,
        StatusCode::OK
    );
    // Failure to insert the transactional audit must roll back the security change.
    sqlx::query("CREATE FUNCTION fail_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'audit unavailable'; END $$")
        .execute(&f.db).await.unwrap();
    sqlx::query("CREATE TRIGGER fail_audit BEFORE INSERT ON audit_log FOR EACH ROW EXECUTE FUNCTION fail_audit()")
        .execute(&f.db).await.unwrap();
    assert_eq!(
        f.request(
            "PATCH",
            &path,
            &f.admin_cookie,
            Some(json!({"name":"Must roll back"}))
        )
        .await
        .0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        vda_server::db::cluster(&f.db, f.cluster)
            .await
            .unwrap()
            .name,
        "Renamed"
    );
    sqlx::query("DROP TRIGGER fail_audit ON audit_log")
        .execute(&f.db)
        .await
        .unwrap();
    f.close().await;
}

#[tokio::test]
async fn approval_lists_recovery_cidrs_and_metrics_are_enforced() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let long = "λ".repeat(2100);
    let id = f.approval(&long, "executed").await;
    sqlx::query("UPDATE approvals SET result=$2,error='failure diagnostic' WHERE id=$1")
        .bind(id)
        .bind(json!({"large":"result"}))
        .execute(&f.db)
        .await
        .unwrap();
    f.approval("SELECT 1", "failed").await;
    let current = f.approval("SELECT 2", "executing").await;
    let stale = f.approval("SELECT 3", "executing").await;
    sqlx::query("UPDATE approvals SET expires_at=now()-interval '1 second',execution_started_at=now() WHERE status='executing'").execute(&f.db).await.unwrap();
    sqlx::query(
        "UPDATE approvals SET execution_started_at=now()-interval '20 minutes' WHERE id=$1",
    )
    .bind(stale)
    .execute(&f.db)
    .await
    .unwrap();
    // The claim used a ten-minute budget; the current one-minute policy must
    // not shorten that claim's recovery window.
    sqlx::query("UPDATE approvals SET execution_started_at=now()-interval '7 minutes' WHERE id=$1")
        .bind(current)
        .execute(&f.db)
        .await
        .unwrap();
    let mut cursor = None;
    let mut seen = Vec::new();
    loop {
        let path = match cursor {
            Some(ref c) => format!("/approvals?limit=1&cursor={c}"),
            None => "/approvals?limit=1".into(),
        };
        let (s, v) = f.request("GET", &path, &f.admin_cookie, None).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        for item in v["items"].as_array().unwrap() {
            assert!(item["result"].is_null());
            seen.push(item["id"].as_str().unwrap().to_owned());
            if item["id"] == json!(id) {
                assert_eq!(item["sql"].as_str().unwrap().chars().count(), 2000);
                assert_eq!(item["sql_truncated"], true);
            }
        }
        cursor = v["next_cursor"].as_str().map(str::to_owned);
        if cursor.is_none() {
            break;
        }
    }
    let unique = seen.iter().collect::<std::collections::HashSet<_>>();
    assert_eq!(seen.len(), 4);
    assert_eq!(unique.len(), 4);
    let (s, v) = f
        .request("GET", &format!("/approvals/{id}"), &f.admin_cookie, None)
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["sql"], long);
    assert_eq!(v["sql_truncated"], false);
    assert_eq!(v["result"]["large"], "result");
    assert_eq!(
        f.request("GET", "/approvals?limit=101", &f.admin_cookie, None)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        f.request("GET", "/approvals?cursor=invalid", &f.admin_cookie, None)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let (_, v) = f
        .request("GET", "/approvals?status=executing", &f.admin_cookie, None)
        .await;
    assert_eq!(v["items"].as_array().unwrap().len(), 2);
    vda_server::health::recover_executions(&f.db).await.unwrap();
    let statuses: Vec<(Uuid, String, Option<String>)> = sqlx::query_as(
        "SELECT id,status,error FROM approvals WHERE status IN ('executing','failed')",
    )
    .fetch_all(&f.db)
    .await
    .unwrap();
    assert!(statuses
        .iter()
        .any(|(id, s, _)| *id == current && s == "executing"));
    assert!(statuses.iter().any(|(id, s, e)| *id == stale
        && s == "failed"
        && e.as_deref() == Some("execution outcome unknown (node crashed or lost)")));
    let pending = f.approval("SELECT 4", "pending").await;
    let approved = f.approval("SELECT 5", "approved").await;
    let mut p = Policy::for_environment("development");
    p.allowed_cidrs = vec!["203.0.113.0/24".into()];
    f.policy(&p).await;
    for (path, body) in [
        (format!("/approvals/{pending}/approve"), Some(json!({}))),
        (
            format!("/approvals/{pending}/reject"),
            Some(json!({"note":"Rejected"})),
        ),
        (format!("/approvals/{approved}/execute"), None),
        (format!("/clusters/{}/health/check", f.cluster), None),
    ] {
        let (s, v) = f.request("POST", &path, &f.admin_cookie, body).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{path}: {v}");
    }
    assert_eq!(
        f.request(
            "GET",
            &format!("/clusters/{}/schema", f.cluster),
            &f.admin_cookie,
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let status: String = sqlx::query_scalar("SELECT status FROM approvals WHERE id=$1")
        .bind(approved)
        .fetch_one(&f.db)
        .await
        .unwrap();
    assert_eq!(status, "approved");
    assert_eq!(
        call(&f.app, "GET", "/metrics", "", None, None, "127.0.0.1:1234")
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &f.app,
            "GET",
            "/metrics",
            "",
            None,
            Some("Bearer wrong"),
            "127.0.0.1:1234"
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &f.app,
            "GET",
            "/metrics",
            "",
            None,
            Some("Bearer metrics-secret"),
            "127.0.0.1:1234"
        )
        .await
        .0,
        StatusCode::OK
    );
    sqlx::query("UPDATE settings SET value=$1 WHERE key='network'")
        .bind(json!({"allowed_cidrs":["203.0.113.0/24"],"trust_proxy_headers":false}))
        .execute(&f.db)
        .await
        .unwrap();
    f.state.invalidate_network().await;
    assert_eq!(
        call(
            &f.app,
            "GET",
            "/metrics",
            "",
            None,
            Some("Bearer metrics-secret"),
            "127.0.0.1:1234"
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    f.close().await;
}

#[tokio::test]
async fn query_intent_is_durable_unique_fail_closed_and_masks_diagnostics() {
    let Ok(target_url) = std::env::var("VDA_TEST_POSTGRES_URL") else {
        eprintln!("skipped: VDA_TEST_POSTGRES_URL is not set");
        return;
    };
    let Some(f) = Fixture::new().await else {
        return;
    };
    let target = sqlx::PgPool::connect(&target_url).await.unwrap();
    let table = format!("vda_harden_data_{}", Uuid::new_v4().simple());
    sqlx::query(&format!(
        "CREATE TABLE {table}(id integer PRIMARY KEY,email text)"
    ))
    .execute(&target)
    .await
    .unwrap();
    sqlx::query(&format!("INSERT INTO {table} VALUES (1,'secret-address')"))
        .execute(&target)
        .await
        .unwrap();
    let query_path = format!("/clusters/{}/query", f.cluster);
    let handle = Uuid::new_v4();
    for _ in 0..2 {
        let (s, v) = f
            .request(
                "POST",
                &query_path,
                &f.admin_cookie,
                Some(json!({"sql":format!("SELECT * FROM {table}"),"query_id":handle})),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        assert_eq!(v["query_id"], json!(handle));
    }
    let ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM query_history")
        .fetch_all(&f.db)
        .await
        .unwrap();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
    assert!(!ids.contains(&handle));
    let (s, v) = f
        .request(
            "POST",
            &query_path,
            &f.admin_cookie,
            Some(json!({"sql":format!("SELECT * FROM {table}"),"query_id":ids[0]})),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM query_history")
        .fetch_one(&f.db)
        .await
        .unwrap();
    assert_eq!(count, 3);
    for body in [
        json!({"sql":"SELECT 1\u{0}"}),
        json!({"sql":"SELECT\u{1}1"}),
    ] {
        assert_eq!(
            f.request("POST", &query_path, &f.admin_cookie, Some(body))
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    let (s, v) = f
        .request(
            "POST",
            &query_path,
            &f.admin_cookie,
            Some(json!({"sql":format!("DROP TABLE {table}")})),
        )
        .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    let blocked: (String, String, String) = sqlx::query_as(
        "SELECT status,sql,host(ip) FROM query_history ORDER BY created_at DESC,id DESC LIMIT 1",
    )
    .fetch_one(&f.db)
    .await
    .unwrap();
    assert_eq!(blocked.0, "blocked");
    assert_eq!(blocked.1, format!("DROP TABLE {table}"));
    assert_eq!(blocked.2, "127.0.0.1");
    // Inject a metadata INSERT failure; the authorized target write must never run.
    sqlx::query("CREATE FUNCTION fail_history() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'history unavailable'; END $$").execute(&f.db).await.unwrap();
    sqlx::query("CREATE TRIGGER fail_history BEFORE INSERT ON query_history FOR EACH ROW EXECUTE FUNCTION fail_history()").execute(&f.db).await.unwrap();
    let (s, v) = f
        .request(
            "POST",
            &query_path,
            &f.admin_cookie,
            Some(json!({"sql":format!("UPDATE {table} SET email='changed' WHERE id=1")})),
        )
        .await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    assert_eq!(v["error"]["code"], "internal");
    let email: String = sqlx::query_scalar(&format!("SELECT email FROM {table} WHERE id=1"))
        .fetch_one(&target)
        .await
        .unwrap();
    assert_eq!(email, "secret-address");
    assert!(f.state.active.is_empty());
    sqlx::query("DROP TRIGGER fail_history ON query_history")
        .execute(&f.db)
        .await
        .unwrap();
    // Intent survives while the actual target statement is blocked on a lock.
    let mut p = Policy::for_environment("development");
    p.require_approval_for_writes = false;
    p.lock_timeout_ms = 10000;
    f.policy(&p).await;
    let mut lock = target.begin().await.unwrap();
    sqlx::query(&format!("LOCK TABLE {table} IN ACCESS EXCLUSIVE MODE"))
        .execute(&mut *lock)
        .await
        .unwrap();
    let sql = format!("SELECT id,email FROM {table}");
    let active = Uuid::new_v4();
    let app = f.app.clone();
    let cookie = f.admin_cookie.clone();
    let path = format!("/api/v1{query_path}");
    let sql2 = sql.clone();
    let running = tokio::spawn(async move {
        call(
            &app,
            "POST",
            &path,
            &cookie,
            Some(json!({"sql":sql2,"query_id":active})),
            None,
            "127.0.0.1:1234",
        )
        .await
    });
    let mut intent = None;
    for _ in 0..200 {
        intent = sqlx::query_as::<_, (Uuid, String)>(
            "SELECT id,status FROM query_history WHERE sql=$1 ORDER BY created_at DESC LIMIT 1",
        )
        .bind(&sql)
        .fetch_optional(&f.db)
        .await
        .unwrap();
        if intent
            .as_ref()
            .is_some_and(|(_, status)| status == "running")
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let (history_id, status) = intent.unwrap();
    assert_eq!(status, "running");
    assert_ne!(history_id, active);
    assert_eq!(
        f.request(
            "POST",
            &format!("/queries/{active}/cancel"),
            &f.admin_cookie,
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(running.await.unwrap().0, StatusCode::BAD_GATEWAY);
    lock.rollback().await.unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM query_history WHERE id=$1")
        .bind(history_id)
        .fetch_one(&f.db)
        .await
        .unwrap();
    assert_eq!(status, "cancelled");
    // Recovery preserves recent running records and retires only abandoned ones.
    sqlx::query("UPDATE query_history SET status='running',created_at=now()-interval '16 minutes' WHERE id=$1").bind(history_id).execute(&f.db).await.unwrap();
    sqlx::query("UPDATE query_history SET status='running' WHERE id=$1")
        .bind(ids[0])
        .execute(&f.db)
        .await
        .unwrap();
    vda_server::health::recover_executions(&f.db).await.unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM query_history WHERE id=$1")
        .bind(history_id)
        .fetch_one(&f.db)
        .await
        .unwrap();
    assert_eq!(status, "unknown");
    let status: String = sqlx::query_scalar("SELECT status FROM query_history WHERE id=$1")
        .bind(ids[0])
        .fetch_one(&f.db)
        .await
        .unwrap();
    assert_eq!(status, "running");
    let (_, v) = f
        .request("GET", "/history?status=unknown", &f.admin_cookie, None)
        .await;
    assert_eq!(v["items"].as_array().unwrap().len(), 1);
    p.masked_columns = vec![format!("{table}.email")];
    f.policy(&p).await;
    let (s, v) = f
        .request(
            "POST",
            &query_path,
            &f.admin_cookie,
            Some(json!({"sql":format!("SELECT email::integer FROM {table}")})),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_GATEWAY, "{v}");
    let hidden = "Query failed (details hidden because the query touches masked data)";
    assert_eq!(v["error"]["message"], hidden);
    let error: String =
        sqlx::query_scalar("SELECT error FROM query_history ORDER BY created_at DESC LIMIT 1")
            .fetch_one(&f.db)
            .await
            .unwrap();
    assert_eq!(error, hidden);
    for sql in [
        format!("SELECT row_to_json(u) FROM {table} u"),
        format!("SELECT u FROM {table} u"),
    ] {
        let (s, v) = f
            .request(
                "POST",
                &query_path,
                &f.admin_cookie,
                Some(json!({"sql":sql})),
            )
            .await;
        assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
        assert_eq!(v["error"]["code"], "query_denied");
        assert!(v["error"]["details"]["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["code"] == "masked_data_serialization"));
    }
    let (s, v) = f
        .request(
            "POST",
            &query_path,
            &f.admin_cookie,
            Some(json!({"sql":format!("SELECT * FROM {table}")})),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["rows"][0][1], "••••••");
    p.max_cost = Some(0.000001);
    f.policy(&p).await;
    let (s, v) = f
        .request(
            "POST",
            &query_path,
            &f.admin_cookie,
            Some(json!({"sql":format!("SELECT * FROM {table}")})),
        )
        .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert_eq!(v["error"]["code"], "cost_exceeded");
    let status: String =
        sqlx::query_scalar("SELECT status FROM query_history ORDER BY created_at DESC LIMIT 1")
            .fetch_one(&f.db)
            .await
            .unwrap();
    assert_eq!(status, "blocked");
    p.max_cost = None;
    p.require_approval_for_writes = true;
    f.policy(&p).await;
    let (s, v) = f
        .request(
            "POST",
            &query_path,
            &f.admin_cookie,
            Some(json!({"sql":format!("UPDATE {table} SET email='new' WHERE id=1")})),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["error"]["code"], "approval_required");
    let id = f
        .approval(
            &format!("UPDATE {table} SET email='changed' WHERE id=999 AND email::integer=1"),
            "approved",
        )
        .await;
    // Use a guaranteed failing approved read to exercise the real failed wire state.
    sqlx::query("UPDATE approvals SET sql=$2 WHERE id=$1")
        .bind(id)
        .bind(format!("SELECT email::integer FROM {table}"))
        .execute(&f.db)
        .await
        .unwrap();
    assert_eq!(
        f.request(
            "POST",
            &format!("/approvals/{id}/execute"),
            &f.admin_cookie,
            None
        )
        .await
        .0,
        StatusCode::BAD_GATEWAY
    );
    let (s, v) = f
        .request("GET", &format!("/approvals/{id}"), &f.admin_cookie, None)
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["status"], "failed");
    assert_eq!(v["error"], hidden);
    assert_eq!(
        f.request(
            "POST",
            &format!("/approvals/{id}/execute"),
            &f.admin_cookie,
            None
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    sqlx::query(&format!("DROP TABLE {table}"))
        .execute(&target)
        .await
        .unwrap();
    target.close().await;
    f.close().await;
}

#[tokio::test]
async fn audit_writer_skips_permanent_errors_and_classifies_serialization_failures() {
    use vda_server::audit::{persist, transient, Event};
    let Some(f) = Fixture::new().await else {
        return;
    };
    let mut event = Event {
        id: Uuid::new_v4(),
        actor_id: Some(f.admin),
        action: "bad\u{0}".into(),
        target_type: None,
        target_id: None,
        ip: None,
        details: json!({"payload":"complete dead letter"}),
        created_at: chrono::Utc::now(),
    };
    let error = persist(&f.db, &event).await.unwrap_err();
    assert!(!transient(&error));
    f.state.audit_sender.send(event.clone()).await.unwrap();
    event.id = Uuid::new_v4();
    event.action = "after.permanent.failure".into();
    f.state.audit_sender.send(event.clone()).await.unwrap();
    let mut written = false;
    for _ in 0..100 {
        written = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM audit_log WHERE id=$1)")
            .bind(event.id)
            .fetch_one(&f.db)
            .await
            .unwrap();
        if written {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(
        written,
        "a permanent data error must not poison subsequent audit delivery"
    );
    sqlx::query("CREATE FUNCTION serialization_error() RETURNS void LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'retry me' USING ERRCODE='40001'; END $$").execute(&f.db).await.unwrap();
    let error = sqlx::query("SELECT serialization_error()")
        .execute(&f.db)
        .await
        .unwrap_err();
    assert!(transient(&error));
    assert!(transient(&sqlx::Error::PoolTimedOut));
    assert!(!transient(&sqlx::Error::RowNotFound));
    f.close().await;
}

#[tokio::test]
async fn account_lock_applies_across_source_ips_and_dns_is_checked_on_cached_health_pools() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    for i in 1..=10 {
        let (s, v) = call(
            &f.app,
            "POST",
            "/api/v1/auth/login",
            "",
            Some(json!({"email":"ADMIN@HARDEN.EXAMPLE","password":"wrong-password"})),
            None,
            &format!("203.0.113.{i}:1234"),
        )
        .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "{v}");
    }
    let (s, v) = call(
        &f.app,
        "POST",
        "/api/v1/auth/login",
        "",
        Some(json!({"email":"admin@harden.example","password":"dummy-password-never-used"})),
        None,
        "203.0.113.20:1234",
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "{v}");
    assert_eq!(
        v["error"]["message"],
        "Authentication required or invalid credentials"
    );
    f.state.login_succeeded("admin@harden.example").await;
    assert_eq!(
        call(
            &f.app,
            "POST",
            "/api/v1/auth/login",
            "",
            Some(json!({"email":"admin@harden.example","password":"dummy-password-never-used"})),
            None,
            "203.0.113.21:1234"
        )
        .await
        .0,
        StatusCode::OK
    );
    if std::env::var_os("VDA_TEST_POSTGRES_URL").is_some() {
        let c = vda_server::db::cluster(&f.db, f.cluster).await.unwrap();
        let p = vda_server::db::policy(&f.db, f.cluster).await.unwrap();
        f.state.pools.get(&f.state, &c, &p).await.unwrap();
        // Keep the pool's version key to simulate an endpoint whose DNS changed
        // after a pool was cached. Health must validate even on a cache hit.
        sqlx::query("UPDATE clusters SET host='169.254.169.254' WHERE id=$1")
            .bind(f.cluster)
            .execute(&f.db)
            .await
            .unwrap();
        let c = vda_server::db::cluster(&f.db, f.cluster).await.unwrap();
        let health = vda_server::health::probe(&f.state, &c).await.unwrap();
        assert_eq!(health.status, "down");
    }
    f.close().await;
}

#[tokio::test]
async fn replica_health_is_reused_for_five_seconds_then_refreshed() {
    if std::env::var_os("VDA_TEST_POSTGRES_URL").is_none() {
        eprintln!("skipped: VDA_TEST_POSTGRES_URL is not set");
        return;
    }
    let Some(f) = Fixture::new().await else {
        return;
    };
    sqlx::query("UPDATE clusters SET replica_host=host,replica_port=port WHERE id=$1")
        .bind(f.cluster)
        .execute(&f.db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO health_checks(cluster_id,endpoint,status) VALUES ($1,'replica','healthy')",
    )
    .bind(f.cluster)
    .execute(&f.db)
    .await
    .unwrap();
    let path = format!("/clusters/{}/query", f.cluster);
    let (s, v) = f
        .request(
            "POST",
            &path,
            &f.admin_cookie,
            Some(json!({"sql":"SELECT 1"})),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["routed_to"], "replica");
    // Remove the metadata source temporarily: a cache hit still executes. Once
    // TTL elapses the missing source must fail closed instead of using stale health.
    sqlx::query("ALTER TABLE health_checks RENAME TO hidden_health_checks")
        .execute(&f.db)
        .await
        .unwrap();
    let (s, v) = f
        .request(
            "POST",
            &path,
            &f.admin_cookie,
            Some(json!({"sql":"SELECT 2"})),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["routed_to"], "replica");
    tokio::time::sleep(std::time::Duration::from_millis(5100)).await;
    assert_eq!(
        f.request(
            "POST",
            &path,
            &f.admin_cookie,
            Some(json!({"sql":"SELECT 3"}))
        )
        .await
        .0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    sqlx::query("ALTER TABLE hidden_health_checks RENAME TO health_checks")
        .execute(&f.db)
        .await
        .unwrap();
    sqlx::query("UPDATE health_checks SET status='down'")
        .execute(&f.db)
        .await
        .unwrap();
    let (s, v) = f
        .request(
            "POST",
            &path,
            &f.admin_cookie,
            Some(json!({"sql":"SELECT 4"})),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["routed_to"], "primary");
    f.close().await;
}

#[tokio::test]
async fn grant_directory_is_paginated_filtered_and_scoped_to_administered_scopes() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let empty_project = Uuid::new_v4();
    sqlx::query("INSERT INTO projects(id,name) VALUES ($1,'Empty project')")
        .bind(empty_project)
        .execute(&f.db)
        .await
        .unwrap();
    for (scope, scope_id, user_id, level) in [
        ("project", empty_project, f.recipient, "read"),
        ("project", f.project, f.recipient, "read"),
        ("cluster", f.cluster, f.recipient, "write"),
    ] {
        sqlx::query("INSERT INTO grants(id,user_id,scope,scope_id,level) VALUES ($1,$2,$3,$4,$5)")
            .bind(Uuid::new_v4())
            .bind(user_id)
            .bind(scope)
            .bind(scope_id)
            .bind(level)
            .execute(&f.db)
            .await
            .unwrap();
    }
    sqlx::query("UPDATE grants SET created_at='2026-01-01T00:00:00Z'")
        .execute(&f.db)
        .await
        .unwrap();
    let (s, first) = f
        .request("GET", "/grants?limit=2", &f.admin_cookie, None)
        .await;
    assert_eq!(s, StatusCode::OK, "{first}");
    assert_eq!(first["items"].as_array().unwrap().len(), 2);
    let cursor = first["next_cursor"].as_str().unwrap();
    let (s, second) = f
        .request(
            "GET",
            &format!("/grants?limit=2&cursor={cursor}"),
            &f.admin_cookie,
            None,
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{second}");
    assert_eq!(second["items"].as_array().unwrap().len(), 2);
    assert!(second["next_cursor"].is_null());
    for a in first["items"].as_array().unwrap() {
        assert!(!second["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|b| a["id"] == b["id"]));
    }
    let (s, scoped) = f.request("GET", "/grants", &f.grantor_cookie, None).await;
    assert_eq!(s, StatusCode::OK, "{scoped}");
    assert_eq!(scoped["items"].as_array().unwrap().len(), 2);
    assert!(scoped["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|g| g["scope"] == "cluster"));
    let (s, filtered) = f
        .request(
            "GET",
            &format!(
                "/grants?scope=project&scope_id={empty_project}&user_id={}",
                f.recipient
            ),
            &f.admin_cookie,
            None,
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{filtered}");
    assert_eq!(filtered["items"][0]["scope_name"], "Empty project");
    assert_eq!(filtered["items"].as_array().unwrap().len(), 1);
    // Project administration includes child clusters, even with no direct cluster grant.
    sqlx::query("UPDATE grants SET scope='project',scope_id=$1 WHERE user_id=$2")
        .bind(f.project)
        .bind(f.grantor)
        .execute(&f.db)
        .await
        .unwrap();
    let (s, scoped) = f.request("GET", "/grants", &f.grantor_cookie, None).await;
    assert_eq!(s, StatusCode::OK, "{scoped}");
    assert_eq!(scoped["items"].as_array().unwrap().len(), 3);
    let recipient_cookie = Fixture::session(&f.db, f.recipient).await;
    assert_eq!(
        f.request("GET", "/grants", &recipient_cookie, None).await.0,
        StatusCode::FORBIDDEN
    );
    for query in [
        "limit=0",
        "limit=201",
        "cursor=garbage",
        "scope=org",
        "scope_id=garbage",
        "user_id=garbage",
    ] {
        let (s, error) = f
            .request("GET", &format!("/grants?{query}"), &f.admin_cookie, None)
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{query}: {error}");
        assert_eq!(error["error"]["code"], "validation");
    }
    sqlx::query("UPDATE grants SET expires_at=now()-interval '1 second' WHERE user_id=$1")
        .bind(f.grantor)
        .execute(&f.db)
        .await
        .unwrap();
    assert_eq!(
        f.request("GET", "/grants", &f.grantor_cookie, None).await.0,
        StatusCode::FORBIDDEN
    );
    f.close().await;
}

#[tokio::test]
async fn user_lookup_is_minimal_bounded_and_requires_unexpired_scope_admin() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    for n in 0..25 {
        sqlx::query("INSERT INTO users(id,email,name,password_hash,org_role,disabled) VALUES ($1,$2,$3,'unused','member',$4)")
            .bind(Uuid::new_v4()).bind(format!("picker{n}@example.com")).bind(format!("Picker {n:02}"))
            .bind(n == 0).execute(&f.db).await.unwrap();
    }
    let (s, found) = f
        .request("GET", "/users/lookup?q=PiCkEr", &f.grantor_cookie, None)
        .await;
    assert_eq!(s, StatusCode::OK, "{found}");
    let items = found["items"].as_array().unwrap();
    assert_eq!(items.len(), 20);
    assert!(items
        .iter()
        .all(|u| u.as_object().unwrap().len() == 3 && u["name"] != "Picker 00"));
    assert_eq!(items[0]["name"], "Picker 01");
    // Wildcards are literal search characters, not a way to enumerate the directory.
    assert!(f
        .request("GET", "/users/lookup?q=%25%25", &f.admin_cookie, None)
        .await
        .1["items"]
        .as_array()
        .unwrap()
        .is_empty());
    for query in ["", "?q=x", "?q=%00aa"] {
        let (s, error) = f
            .request(
                "GET",
                &format!("/users/lookup{query}"),
                &f.admin_cookie,
                None,
            )
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{error}");
        assert_eq!(error["error"]["code"], "validation");
    }
    let recipient_cookie = Fixture::session(&f.db, f.recipient).await;
    assert_eq!(
        f.request("GET", "/users/lookup?q=Picker", &recipient_cookie, None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        f.request("GET", "/users", &f.grantor_cookie, None).await.0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("UPDATE grants SET expires_at=now()-interval '1 second' WHERE user_id=$1")
        .bind(f.grantor)
        .execute(&f.db)
        .await
        .unwrap();
    assert_eq!(
        f.request("GET", "/users/lookup?q=Picker", &f.grantor_cookie, None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    f.close().await;
}

#[tokio::test]
async fn master_key_change_reports_unreadable_credentials_and_new_password_recovers() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    // Simulate ciphertext written under a previous VDA_MASTER_KEY.
    let target_url = std::env::var("VDA_TEST_POSTGRES_URL")
        .unwrap_or_else(|_| "postgres://vda@127.0.0.1:9/postgres".into());
    let password = target_url
        .split_once("://")
        .and_then(|(_, s)| s.split_once('@'))
        .and_then(|(s, _)| s.split_once(':'))
        .map(|(_, p)| p)
        .unwrap_or("");
    let stale = Crypto::new(&[9; 32]).encrypt(f.cluster, password).unwrap();
    sqlx::query("UPDATE clusters SET password_enc=$1 WHERE id=$2")
        .bind(stale)
        .bind(f.cluster)
        .execute(&f.db)
        .await
        .unwrap();
    let check = format!("/clusters/{}/health/check", f.cluster);
    let (s, v) = f.request("POST", &check, &f.admin_cookie, None).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "down");
    assert!(
        v["error"].as_str().unwrap().contains("can't be decrypted"),
        "{v}"
    );
    let (s, v) = f
        .request(
            "POST",
            &format!("/clusters/{}/query", f.cluster),
            &f.admin_cookie,
            Some(json!({"sql":"SELECT 1"})),
        )
        .await;
    assert_eq!(s, StatusCode::INTERNAL_SERVER_ERROR, "{v}");
    assert_eq!(v["error"]["code"], "credentials_unreadable");
    // Re-entering the password must not require decrypting the stale ciphertext.
    let (s, v) = f
        .request(
            "PATCH",
            &format!("/clusters/{}", f.cluster),
            &f.admin_cookie,
            Some(json!({"password": password})),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    if std::env::var("VDA_TEST_POSTGRES_URL").is_ok() {
        let (s, v) = f.request("POST", &check, &f.admin_cookie, None).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        assert_ne!(v["status"], "down", "{v}");
    }
    f.close().await;
}
