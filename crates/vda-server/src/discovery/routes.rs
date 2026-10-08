use super::{resources, scan, sources};
use crate::app::AppState;
use axum::{
    routing::{get, patch, post},
    Router,
};

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/discovery/sources",
            get(sources::list).post(sources::create),
        )
        .route(
            "/discovery/sources/{id}",
            patch(sources::update).delete(sources::delete),
        )
        .route("/discovery/sources/test", post(sources::test_draft))
        .route("/discovery/sources/{id}/test", post(sources::test))
        .route("/discovery/sources/{id}/scan", post(scan::run_route))
        .route("/discovery/sources/{id}/runs", get(scan::runs))
        .route("/discovery/resources", get(resources::list))
        .route("/discovery/resources/{id}/import", post(resources::import))
        .route("/discovery/resources/{id}/ignore", post(resources::ignore))
        .route(
            "/discovery/resources/{id}/unignore",
            post(resources::unignore),
        )
        .route("/discovery/resources/{id}/sync", post(resources::sync))
}
