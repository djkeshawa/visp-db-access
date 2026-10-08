//! Bounded administration directories without exposing credentials or unrelated grants.
use crate::{admin::GRANT_SQL, app::AppState, auth::User, db, error::ApiError};
use axum::{
    extract::{rejection::QueryRejection, Query, State},
    Extension, Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

// A cluster administrator can manage cluster grants, but cannot administer the
// parent project's grants. Project administrators also administer child clusters.
const ADMIN_SCOPE_SQL: &str = "EXISTS (
    SELECT 1 FROM grants own WHERE own.user_id=$1 AND own.level='admin'
    AND (own.expires_at IS NULL OR own.expires_at>now())
    AND ((own.scope=g.scope AND own.scope_id=g.scope_id)
      OR (g.scope='cluster' AND own.scope='project' AND own.scope_id=
          (SELECT project_id FROM clusters WHERE id=g.scope_id))))";

async fn require_scope_admin(state: &AppState, user: &User) -> Result<(), ApiError> {
    if user.org_role == "admin" {
        return Ok(());
    }
    let permitted: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM grants g WHERE user_id=$1 AND level='admin'
         AND (expires_at IS NULL OR expires_at>now())
         AND ((scope='project' AND EXISTS(SELECT 1 FROM projects WHERE id=g.scope_id))
           OR (scope='cluster' AND EXISTS(SELECT 1 FROM clusters WHERE id=g.scope_id))))",
    )
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;
    if permitted {
        Ok(())
    } else {
        Err(ApiError::forbidden())
    }
}

#[derive(Deserialize)]
pub(crate) struct GrantFilters {
    scope: Option<String>,
    scope_id: Option<Uuid>,
    user_id: Option<Uuid>,
    limit: Option<i64>,
    cursor: Option<String>,
}

pub(crate) async fn grants(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    query: Result<Query<GrantFilters>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    require_scope_admin(&state, &user).await?;
    let Query(filters) = query.map_err(|_| ApiError::validation("Invalid query parameters"))?;
    if filters
        .scope
        .as_deref()
        .is_some_and(|s| !matches!(s, "project" | "cluster"))
    {
        return Err(ApiError::validation("Invalid grant scope"));
    }
    let (limit, cursor) = db::Page {
        limit: filters.limit,
        cursor: filters.cursor,
        ..Default::default()
    }
    .bounds()?;
    let items: Vec<Value> = sqlx::query_scalar(&format!(
        "{GRANT_SQL} WHERE ($2 OR {ADMIN_SCOPE_SQL})
         AND ($3::text IS NULL OR g.scope=$3) AND ($4::uuid IS NULL OR g.scope_id=$4)
         AND ($5::uuid IS NULL OR g.user_id=$5)
         AND ($6::timestamptz IS NULL OR (g.created_at,g.id)<($6,$7))
         ORDER BY g.created_at DESC,g.id DESC LIMIT $8"
    ))
    .bind(user.id)
    .bind(user.org_role == "admin")
    .bind(filters.scope)
    .bind(filters.scope_id)
    .bind(filters.user_id)
    .bind(cursor.as_ref().map(|c| c.created_at))
    .bind(cursor.as_ref().map(|c| c.id))
    .bind(limit + 1)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(db::page(items, limit)?))
}

#[derive(Deserialize)]
pub(crate) struct Lookup {
    q: String,
}

pub(crate) async fn users(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    query: Result<Query<Lookup>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    require_scope_admin(&state, &user).await?;
    let Query(lookup) = query.map_err(|_| ApiError::validation("Invalid query parameters"))?;
    crate::sanitation::text(&lookup.q)?;
    let q = lookup.q.trim();
    if !(2..=254).contains(&q.chars().count()) {
        return Err(ApiError::validation(
            "Search must contain 2..=254 characters",
        ));
    }
    // strpos treats percent, underscore and backslash literally, unlike LIKE.
    let items: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',id,'email',email,'name',name) FROM users
         WHERE NOT disabled AND (strpos(lower(email),lower($1))>0 OR strpos(lower(name),lower($1))>0)
         ORDER BY lower(name),lower(email),id LIMIT 20",
    ).bind(q).fetch_all(&state.db).await?;
    Ok(Json(json!({"items":items})))
}
