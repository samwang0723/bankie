use std::collections::HashSet;
use std::sync::Arc;

use axum::{routing::get, Json, Router};
use chrono::{Duration, Utc};

use bankie_common::error::AppError;

use crate::models::api_key::KeyStatus;
use crate::models::auth::SessionClaims;
use crate::models::dashboard::AuditLogEntry;
use crate::state::PortalState;

/// Protected dashboard routes (session auth required).
pub fn dashboard_routes() -> Router<Arc<PortalState>> {
    Router::new()
        .route("/dashboard/stats", get(stats))
        .route("/dashboard/activity", get(activity))
}

/// GET /portal/v1/dashboard/stats
///
/// Returns dashboard statistics for the current org:
/// - active_api_keys: count of active keys
/// - total_api_keys: count of all keys
/// - scopes_granted: unique scope count across all active keys
/// - total_requests_today: API calls in the last 24 hours
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

    // Compute unique scopes across all active API keys
    let scopes_granted = compute_unique_scopes(&keys);

    // Count API calls in the last 24 hours
    let since = Utc::now() - Duration::hours(24);
    let total_requests_today = state
        .dashboard_repo
        .count_api_calls_since(org_id, since)
        .await
        .unwrap_or(0);

    Ok(Json(serde_json::json!({
        "total_api_keys": total_api_keys,
        "active_api_keys": active_api_keys,
        "scopes_granted": scopes_granted,
        "total_requests_today": total_requests_today,
        "org_name": org.name,
        "environment": "live"
    })))
}

/// GET /portal/v1/dashboard/activity
///
/// Returns the 10 most recent audit log entries for the org.
async fn activity(
    state: axum::extract::State<Arc<PortalState>>,
    claims: axum::Extension<SessionClaims>,
) -> Result<Json<Vec<AuditLogEntry>>, AppError> {
    let org_id: uuid::Uuid = claims
        .org_id
        .parse()
        .map_err(|_| AppError::internal("Invalid org_id in session"))?;

    let entries = state
        .dashboard_repo
        .list_recent_activity(org_id, 10)
        .await
        .map_err(AppError::internal)?;

    Ok(Json(entries))
}

/// Count unique scopes across all active API keys.
fn compute_unique_scopes(keys: &[crate::models::api_key::ApiKey]) -> usize {
    let mut unique: HashSet<&str> = HashSet::new();
    for key in keys {
        if key.status == KeyStatus::Active {
            for scope in &key.scopes {
                unique.insert(scope.as_str());
            }
        }
    }
    unique.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::api_key::ApiKey;
    use chrono::Utc;
    use uuid::Uuid;

    fn make_key(status: KeyStatus, scopes: Vec<&str>) -> ApiKey {
        ApiKey {
            id: Uuid::new_v4(),
            org_id: Uuid::new_v4(),
            tenant_id: 1,
            name: "test".to_string(),
            key_prefix: "bk_live_test1234".to_string(),
            key_hash: "hash".to_string(),
            scopes: scopes.into_iter().map(String::from).collect(),
            status,
            grace_expires_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_compute_unique_scopes_empty() {
        assert_eq!(compute_unique_scopes(&[]), 0);
    }

    #[test]
    fn test_compute_unique_scopes_single_key() {
        let keys = vec![make_key(
            KeyStatus::Active,
            vec!["accounts:read", "ledgers:read"],
        )];
        assert_eq!(compute_unique_scopes(&keys), 2);
    }

    #[test]
    fn test_compute_unique_scopes_overlapping() {
        let keys = vec![
            make_key(KeyStatus::Active, vec!["accounts:read", "ledgers:read"]),
            make_key(
                KeyStatus::Active,
                vec!["accounts:read", "transactions:read"],
            ),
        ];
        // accounts:read, ledgers:read, transactions:read = 3 unique
        assert_eq!(compute_unique_scopes(&keys), 3);
    }

    #[test]
    fn test_compute_unique_scopes_ignores_revoked() {
        let keys = vec![
            make_key(KeyStatus::Active, vec!["accounts:read"]),
            make_key(
                KeyStatus::Revoked,
                vec!["ledgers:read", "transactions:read"],
            ),
        ];
        // Only active key's scope counts
        assert_eq!(compute_unique_scopes(&keys), 1);
    }

    #[test]
    fn test_compute_unique_scopes_ignores_rotated() {
        let keys = vec![
            make_key(KeyStatus::Rotated, vec!["accounts:read", "ledgers:read"]),
            make_key(KeyStatus::Active, vec!["accounts:read"]),
        ];
        assert_eq!(compute_unique_scopes(&keys), 1);
    }

    #[test]
    fn test_compute_unique_scopes_no_active_keys() {
        let keys = vec![
            make_key(KeyStatus::Revoked, vec!["accounts:read"]),
            make_key(KeyStatus::Rotated, vec!["ledgers:read"]),
        ];
        assert_eq!(compute_unique_scopes(&keys), 0);
    }
}
