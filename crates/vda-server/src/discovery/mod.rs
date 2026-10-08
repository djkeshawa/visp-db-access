//! Administrative cloud discovery with explicit credential-bearing import.
mod drift;
mod resources;
pub(crate) mod routes;
pub(crate) mod scan;
mod sources;

use crate::{app::AppState, auth::User, error::ApiError, network::ClientIp, rbac};

fn authorize(user: &User) -> Result<(), ApiError> {
    rbac::require_admin(user)
}
async fn gate(state: &AppState, user: &User, ip: ClientIp) -> Result<(), ApiError> {
    authorize(user)?;
    let settings = state.network_settings().await?;
    if !crate::network::allowed(ip.0, &crate::network::parse_cidrs(&settings.allowed_cidrs)?) {
        return Err(ApiError::forbidden());
    }
    Ok(())
}

/// Start the leader-elected cloud discovery scheduler.
pub async fn scheduler(state: AppState) {
    scan::scheduler(state).await;
}
