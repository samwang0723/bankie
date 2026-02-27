use std::sync::Arc;

use axum::{routing::get, Json, Router};

use bankie_common::error::AppError;

use crate::models::api_key::KeyStatus;
use crate::models::auth::SessionClaims;
use crate::state::PortalState;

/// Protected dashboard routes (session auth required).
pub fn dashboard_routes() -> Router<Arc<PortalState>> {
    Router::new().route("/dashboard/stats", get(stats))
}

/// GET /portal/v1/dashboard/stats
///
/// Returns basic dashboard statistics for the current org.
async fn stats(
    state: axum::extract::State<Arc<PortalState>>,
    claims: axum::Extension<SessionClaims>,
) -> Result<Json<serde_json::Value>, AppError> {
    let org_id: uuid::Uuid = claims
        .org_id
        .parse()
        .map_err(|_| AppError::internal("Invalid org_id in session"))?;

    let org = state
        .org_repo
        .find_by_id(org_id)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::NotFound("Organization not found".to_string()))?;

    let keys = state
        .api_key_repo
        .list_by_org(org_id)
        .await
        .map_err(AppError::internal)?;

    let total_api_keys = keys.len();
    let active_api_keys = keys
        .iter()
        .filter(|k| k.status == KeyStatus::Active)
        .count();

    Ok(Json(serde_json::json!({
        "total_api_keys": total_api_keys,
        "active_api_keys": active_api_keys,
        "total_requests_today": 0,
        "org_name": org.name,
        "environment": "live"
    })))
}
