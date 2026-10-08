#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Runs against an isolated temporary schema only when VDA_TEST_DATABASE_URL is set.
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
};

async fn request(
    router: &Router,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value, Option<String>) {
    let mut req = Request::builder()
        .method(method)
        .uri(format!("/api/v1{path}"))
        .header("x-requested-with", "vda");
    if let Some(cookie) = cookie {
        req = req.header("cookie", cookie);
    }
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
    let response = router.clone().oneshot(req).await.unwrap();
    let status = response.status();
    let cookie = response
        .headers()
        .get("set-cookie")
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string());
    let bytes = to_bytes(response.into_body(), 40 * 1024 * 1024)
        .await
        .unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, value, cookie)
}
async fn login(router: &Router, email: &str) -> String {
    let (status, value, cookie) = request(
        router,
        "POST",
        "/auth/login",
        None,
        Some(json!({"email":email,"password":"integration-password"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    cookie.unwrap()
}

#[tokio::test]
async fn authenticated_crud_policy_authorization_and_four_eyes() {
    let Ok(url) = std::env::var("VDA_TEST_DATABASE_URL") else {
        eprintln!("skipped: VDA_TEST_DATABASE_URL is not set");
        return;
    };
    let admin_db = sqlx::PgPool::connect(&url).await.unwrap();
    let schema = format!("vda_server_test_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin_db)
        .await
        .unwrap();
    let search_path = format!("SET search_path TO {schema},public");
    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .after_connect(move |conn, _| {
            let sql = search_path.clone();
            Box::pin(async move {
                sqlx::query(&sql).execute(&mut *conn).await?;
                sqlx::query("SET TIME ZONE 'UTC'").execute(conn).await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .unwrap();
    vda_server::db::migrate(&db).await.unwrap();
    let mut config = Cli::parse_from(["test"]).config;
    config.database_url = Some(url.into());
    config.allow_private_targets = true;
    let (state, writer) = AppState::new(db.clone(), config, Crypto::new(&[9; 32]), None)
        .await
        .unwrap();
    let admin = Uuid::new_v4();
    let hash = vda_server::auth::hash_password("integration-password".into())
        .await
        .unwrap();
    sqlx::query("INSERT INTO users(id,email,name,password_hash,org_role) VALUES ($1,'admin@test.example','Admin',$2,'admin')").bind(admin).bind(&hash).execute(&db).await.unwrap();
    let app = router(state.clone());
    let cookie = login(&app, "admin@test.example").await;
    let (status, me, _) = request(&app, "GET", "/auth/me", Some(&cookie), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["user"]["id"], admin.to_string());
    assert!(me["user"].get("password_hash").is_none());
    let (status, _, _) = request(
        &app,
        "POST",
        "/auth/login",
        None,
        Some(json!({"email":"missing@test.example","password":"wrong"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status,member,_)=request(&app,"POST","/users",Some(&cookie),Some(json!({"email":"member@test.example","name":"Member","password":"integration-password","org_role":"member"}))).await;
    assert_eq!(status, StatusCode::OK, "{member}");
    let member_id = member["id"].as_str().unwrap();
    let member_cookie = login(&app, "member@test.example").await;
    let (_,reviewer,_)=request(&app,"POST","/users",Some(&cookie),Some(json!({"email":"reviewer@test.example","name":"Reviewer","password":"integration-password","org_role":"admin"}))).await;
    assert!(reviewer["id"].is_string());
    let reviewer_cookie = login(&app, "reviewer@test.example").await;
    let (status, project, _) = request(
        &app,
        "POST",
        "/projects",
        Some(&cookie),
        Some(json!({"name":"Shop","description":"Test project"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{project}");
    let project_id = project["id"].as_str().unwrap();
    let (status,cluster,_)=request(&app,"POST","/clusters",Some(&cookie),Some(json!({"project_id":project_id,"name":"Shop DB","engine":"postgres","provider":"onprem","region":"local","environment":"development","host":"127.0.0.1","port":5432,"database":"test","username":"test","password":"target-password","tls_mode":"disable"}))).await;
    assert_eq!(status, StatusCode::OK, "{cluster}");
    assert!(cluster.get("password").is_none());
    assert!(cluster.get("password_enc").is_none());
    assert_eq!(cluster["health"]["status"], "unknown");
    let cluster_id = cluster["id"].as_str().unwrap();
    let path = format!("/clusters/{cluster_id}");
    let (status, _, _) = request(&app, "GET", &path, Some(&member_cookie), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (_, clusters, _) = request(&app, "GET", "/clusters", Some(&member_cookie), None).await;
    assert!(clusters["items"].as_array().unwrap().is_empty());
    let (status, grant, _) = request(
        &app,
        "POST",
        "/grants",
        Some(&cookie),
        Some(json!({"user_id":member_id,"scope":"project","scope_id":project_id,"level":"write"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{grant}");
    assert_eq!(grant["scope_name"], "Shop");
    let (status, c, _) = request(&app, "GET", &path, Some(&member_cookie), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(c["my_access"], "write");
    let (status, updated, _) = request(
        &app,
        "PATCH",
        &path,
        Some(&cookie),
        Some(json!({"name":"Renamed","tags":{"team":"engineering"}})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["name"], "Renamed");
    let (_, mut policy, _) =
        request(&app, "GET", &format!("{path}/policy"), Some(&cookie), None).await;
    policy["max_rows"] = json!(0);
    let (status, _, _) = request(
        &app,
        "PUT",
        &format!("{path}/policy"),
        Some(&cookie),
        Some(policy.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    policy["max_rows"] = json!(100);
    let (status, _, _) = request(
        &app,
        "PUT",
        &format!("{path}/policy"),
        Some(&member_cookie),
        Some(policy.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _, _) = request(
        &app,
        "PUT",
        &format!("{path}/policy"),
        Some(&cookie),
        Some(policy),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    if let Ok(target_url) = std::env::var("VDA_TEST_POSTGRES_URL") {
        let target = sqlx::PgPool::connect(&target_url).await.unwrap();
        let options = target.connect_options();
        let table = format!("vda_pipeline_{}", Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE TABLE {table}(id integer PRIMARY KEY, email text NOT NULL, amount integer NOT NULL)"))
            .execute(&target).await.unwrap();
        sqlx::query(&format!("INSERT INTO {table} VALUES (1,'alice@example.com',10),(2,'bob@example.com',20),(3,'carol@example.com',30)"))
            .execute(&target).await.unwrap();
        let (status, c, _) = request(&app, "POST", "/clusters", Some(&cookie), Some(json!({
            "project_id":project_id,"name":"Live target","engine":"postgres","provider":"onprem","region":"local",
            "environment":"development","host":options.get_host(),"port":options.get_port(),
            "database":options.get_database().unwrap_or("postgres"),"username":options.get_username(),"password":url_password(&target_url),"tls_mode":"disable"
        }))).await;
        assert_eq!(status, StatusCode::OK, "{c}");
        let live = format!("/clusters/{}", c["id"].as_str().unwrap());
        let (_, mut p, _) =
            request(&app, "GET", &format!("{live}/policy"), Some(&cookie), None).await;
        p["max_rows"] = json!(2);
        p["masked_columns"] = json!(["email"]);
        let (status, _, _) = request(
            &app,
            "PUT",
            &format!("{live}/policy"),
            Some(&cookie),
            Some(p.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, result, _) = request(
            &app,
            "POST",
            &format!("{live}/query"),
            Some(&member_cookie),
            Some(json!({"sql":format!("SELECT id,email FROM {table} ORDER BY id")})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{result}");
        assert_eq!(result["row_count"], 2);
        assert_eq!(result["columns"][1]["masked"], true);
        assert_eq!(result["rows"][0][1], "••••••");
        assert_eq!(result["routed_to"], "primary");
        assert!(result["executed_sql"].as_str().unwrap().contains("LIMIT"));
        let (status, denied, _) = request(
            &app,
            "POST",
            &format!("{live}/query"),
            Some(&member_cookie),
            Some(json!({"sql":format!("DROP TABLE {table}")})),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{denied}");
        assert_eq!(denied["error"]["code"], "query_denied");
        let sql = format!("UPDATE {table} SET amount=11 WHERE id=1");
        let (status, _, _) = request(
            &app,
            "POST",
            &format!("{live}/query"),
            Some(&member_cookie),
            Some(json!({"sql":sql})),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        let (status, approval, _) = request(
            &app,
            "POST",
            "/approvals",
            Some(&member_cookie),
            Some(json!({"cluster_id":c["id"],"sql":sql,"reason":"Adjust one row"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{approval}");
        let approval_path = format!("/approvals/{}", approval["id"].as_str().unwrap());
        let (status, _, _) = request(
            &app,
            "POST",
            &format!("{approval_path}/approve"),
            Some(&reviewer_cookie),
            Some(json!({})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, executed, _) = request(
            &app,
            "POST",
            &format!("{approval_path}/execute"),
            Some(&member_cookie),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{executed}");
        assert_eq!(executed["status"], "executed");
        assert_eq!(executed["result"]["affected_rows"], 1);
        let (status, _, _) = request(
            &app,
            "POST",
            &format!("{approval_path}/execute"),
            Some(&member_cookie),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        let amount: i32 = sqlx::query_scalar(&format!("SELECT amount FROM {table} WHERE id=1"))
            .fetch_one(&target)
            .await
            .unwrap();
        assert_eq!(amount, 11);
        let (status, h, _) = request(
            &app,
            "POST",
            &format!("{live}/health/check"),
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{h}");
        assert_eq!(h["status"], "healthy");
        let (status, schema, _) = request(
            &app,
            "GET",
            &format!("{live}/schema"),
            Some(&member_cookie),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{schema}");
        assert!(!schema["schemas"].as_array().unwrap().is_empty());
        // A tiny cost budget must stop the read before execution.
        p["max_cost"] = json!(0.000001);
        request(
            &app,
            "PUT",
            &format!("{live}/policy"),
            Some(&cookie),
            Some(p.clone()),
        )
        .await;
        let (status, cost, _) = request(
            &app,
            "POST",
            &format!("{live}/query"),
            Some(&member_cookie),
            Some(json!({"sql":format!("SELECT * FROM {table}")})),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{cost}");
        assert_eq!(cost["error"]["code"], "cost_exceeded");
        p["max_cost"] = Value::Null;
        p["max_concurrent_queries"] = json!(1);
        p["lock_timeout_ms"] = json!(10000);
        request(
            &app,
            "PUT",
            &format!("{live}/policy"),
            Some(&cookie),
            Some(p),
        )
        .await;
        // Hold a target lock so the read deterministically remains active without
        // using a disruptive function (the guard correctly blocks pg_sleep).
        let mut lock = target.begin().await.unwrap();
        sqlx::query(&format!("LOCK TABLE {table} IN ACCESS EXCLUSIVE MODE"))
            .execute(&mut *lock)
            .await
            .unwrap();
        // Cancellation happens while a real connector query is active.
        let query_id = Uuid::new_v4();
        let app_for_query = app.clone();
        let member_for_query = member_cookie.clone();
        let live_for_query = live.clone();
        let table_for_query = table.clone();
        let running = tokio::spawn(async move {
            request(
                &app_for_query,
                "POST",
                &format!("{live_for_query}/query"),
                Some(&member_for_query),
                Some(json!({"sql":format!("SELECT * FROM {table_for_query}"),"query_id":query_id})),
            )
            .await
        });
        for _ in 0..100 {
            if state.active.contains_key(&query_id) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(state.active.contains_key(&query_id));
        let (status, busy, _) = request(
            &app,
            "POST",
            &format!("{live}/query"),
            Some(&member_cookie),
            Some(json!({"sql":format!("SELECT * FROM {table}")})),
        )
        .await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{busy}");
        assert_eq!(busy["error"]["code"], "busy");
        let (status, _, _) = request(
            &app,
            "POST",
            &format!("/queries/{query_id}/cancel"),
            Some(&member_cookie),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, cancelled, _) = running.await.unwrap();
        lock.rollback().await.unwrap();
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{cancelled}");
        assert_eq!(cancelled["error"]["message"], "Query cancelled");
        let (status, _, _) = request(&app, "DELETE", &live, Some(&cookie), None).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        sqlx::query(&format!("DROP TABLE {table}"))
            .execute(&target)
            .await
            .unwrap();
        target.close().await;
    }
    // Create a review fixture without depending on concurrent guard implementation.
    let approval = Uuid::new_v4();
    let analysis = json!({"verdict":"requires_approval","risk":"high","statements":[],"rewritten_sql":"UPDATE products SET price=1 WHERE id=1","issues":[]});
    sqlx::query("INSERT INTO approvals(id,cluster_id,requester_id,sql,reason,analysis,status) VALUES ($1,$2,$3,'UPDATE products SET price=1 WHERE id=1','Test review',$4,'pending')").bind(approval).bind(cluster_id.parse::<Uuid>().unwrap()).bind(admin).bind(analysis).execute(&db).await.unwrap();
    let approve_path = format!("/approvals/{approval}/approve");
    let (status, _, _) = request(&app, "POST", &approve_path, Some(&cookie), Some(json!({}))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (a, b) = tokio::join!(
        request(
            &app,
            "POST",
            &approve_path,
            Some(&reviewer_cookie),
            Some(json!({"note":"Reviewed"}))
        ),
        request(
            &app,
            "POST",
            &approve_path,
            Some(&reviewer_cookie),
            Some(json!({"note":"Reviewed"}))
        )
    );
    assert!(matches!(
        (a.0, b.0),
        (StatusCode::OK, StatusCode::CONFLICT) | (StatusCode::CONFLICT, StatusCode::OK)
    ));
    let (status, _, _) = request(
        &app,
        "PUT",
        "/settings/network",
        Some(&cookie),
        Some(json!({"allowed_cidrs":["203.0.113.0/24"],"trust_proxy_headers":false})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, overview, _) = request(&app, "GET", "/overview", Some(&cookie), None).await;
    assert_eq!(status, StatusCode::OK, "{overview}");
    assert_eq!(overview["clusters_total"], 1);
    let (status, _, _) = request(
        &app,
        "DELETE",
        &format!("/projects/{project_id}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _, _) = request(
        &app,
        "DELETE",
        &format!("/grants/{}", grant["id"].as_str().unwrap()),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, _) = request(&app, "GET", &path, Some(&member_cookie), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let digest = vda_server::auth::token_hash(member_cookie.strip_prefix("vda_session=").unwrap());
    sqlx::query("UPDATE sessions SET expires_at=now()+interval '1 hour',absolute_expires_at=now()+interval '2 hours' WHERE token_hash=$1").bind(&digest).execute(&db).await.unwrap();
    let (status, _, _) = request(&app, "GET", "/auth/me", Some(&member_cookie), None).await;
    assert_eq!(status, StatusCode::OK);
    let capped: bool = sqlx::query_scalar(
        "SELECT expires_at=absolute_expires_at FROM sessions WHERE token_hash=$1",
    )
    .bind(&digest)
    .fetch_one(&db)
    .await
    .unwrap();
    assert!(capped);
    sqlx::query(
        "UPDATE sessions SET absolute_expires_at=now()-interval '1 second' WHERE token_hash=$1",
    )
    .bind(digest)
    .execute(&db)
    .await
    .unwrap();
    let (status, _, _) = request(&app, "GET", "/auth/me", Some(&member_cookie), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let member_cookie = login(&app, "member@test.example").await;
    let (status, _, _) = request(&app, "POST", "/auth/logout", Some(&member_cookie), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, _) = request(&app, "GET", "/auth/me", Some(&member_cookie), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let member_cookie = login(&app, "member@test.example").await;
    let (status, _, _) = request(
        &app,
        "DELETE",
        &format!("/users/{member_id}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, _) = request(&app, "GET", "/auth/me", Some(&member_cookie), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _, _) = request(&app, "DELETE", &path, Some(&cookie), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, _) = request(
        &app,
        "DELETE",
        &format!("/projects/{project_id}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, changed, _) = request(
        &app,
        "PATCH",
        &format!("/users/{}", reviewer["id"].as_str().unwrap()),
        Some(&cookie),
        Some(json!({"name":"Updated Reviewer","org_role":"member"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(changed["name"], "Updated Reviewer");
    let (status, users, _) = request(&app, "GET", "/users", Some(&cookie), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(users["items"].as_array().unwrap().len(), 3);
    let (status, _, _) = request(&app, "GET", "/users", Some(&reviewer_cookie), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _, _) = request(
        &app,
        "POST",
        "/auth/change-password",
        Some(&cookie),
        Some(json!({"current_password":"incorrect","new_password":"replacement-password"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status,_,_)=request(&app,"POST","/auth/change-password",Some(&cookie),Some(json!({"current_password":"integration-password","new_password":"replacement-password"}))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, _) = request(&app, "GET", "/auth/me", Some(&cookie), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    state.tasks.close();
    state.tasks.wait().await;
    state.stop.cancel();
    writer.await.unwrap();
    let audit_count: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_log")
        .fetch_one(&db)
        .await
        .unwrap();
    assert!(audit_count > 10);
    // Snapshot identities survive target deletion while FK references are cleared.
    if std::env::var_os("VDA_TEST_POSTGRES_URL").is_some() {
        let (history_count, deleted_refs): (i64, i64) = sqlx::query_as(
            "SELECT count(*),count(*) FILTER(WHERE cluster_ref IS NULL) FROM query_history",
        )
        .fetch_one(&db)
        .await
        .unwrap();
        assert!(history_count >= 6);
        assert_eq!(history_count, deleted_refs);
    }

    let immutable = sqlx::query("DELETE FROM audit_log").execute(&db).await;
    assert!(immutable.is_err());
    db.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin_db)
        .await
        .unwrap();
    admin_db.close().await;
}

fn url_password(url: &str) -> String {
    let encoded = url
        .split_once("://")
        .and_then(|(_, rest)| rest.split_once('@'))
        .and_then(|(credentials, _)| credentials.split_once(':'))
        .map(|(_, password)| password)
        .unwrap_or("");
    let mut bytes = encoded.bytes();
    let mut result = Vec::new();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let high = (bytes.next().unwrap() as char).to_digit(16).unwrap();
            let low = (bytes.next().unwrap() as char).to_digit(16).unwrap();
            result.push((high * 16 + low) as u8);
        } else {
            result.push(byte);
        }
    }
    String::from_utf8(result).unwrap()
}
