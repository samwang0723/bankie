use std::sync::Arc;

use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;

use bankie_common::error::AppError;

use crate::models::auth::SessionClaims;
use crate::models::dashboard::AuditLogFilters;
use crate::state::PortalState;

/// Protected audit log routes (session auth required).
pub fn audit_log_routes() -> Router<Arc<PortalState>> {
    Router::new().route("/audit-logs", get(list_audit_logs))
}

/// Query parameters for the audit logs endpoint.
#[derive(Debug, Deserialize)]
pub struct AuditLogQueryParams {
    pub action: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
}

/// GET /portal/v1/audit-logs
///
/// Returns paginated audit logs for the current org with optional filters.
/// Supported filters: action, from/to (time range).
/// Default: offset=0, limit=50, max limit=100.
async fn list_audit_logs(
    state: State<Arc<PortalState>>,
    claims: axum::Extension<SessionClaims>,
    Query(params): Query<AuditLogQueryParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    let org_id: uuid::Uuid = claims
        .org_id
        .parse()
        .map_err(|_| AppError::internal("Invalid org_id in session"))?;

    let offset = params.offset.unwrap_or(0).max(0);
    let limit = params.limit.unwrap_or(50).clamp(1, 100);

    let filters = AuditLogFilters {
        action: params.action,
        from: params.from,
        to: params.to,
    };

    let (entries, total) = state
        .dashboard_repo
        .list_audit_logs(org_id, filters, offset, limit)
        .await
        .map_err(AppError::internal)?;

    Ok(Json(serde_json::json!({
        "data": entries,
        "total": total,
        "offset": offset,
        "limit": limit,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    use crate::models::auth::SessionClaims;
    use crate::models::dashboard::AuditLogEntry;
    use crate::repo::api_key::MockApiKeyRepository;
    use crate::repo::dashboard::MockDashboardRepository;
    use crate::repo::member::MockMemberRepository;
    use crate::repo::org::MockOrgRepository;
    use crate::repo::webhook::MockWebhookRepository;
    use crate::state::PortalState;

    fn make_test_state(dashboard_repo: MockDashboardRepository) -> Arc<PortalState> {
        Arc::new(PortalState {
            org_repo: Arc::new(MockOrgRepository::new()),
            member_repo: Arc::new(MockMemberRepository::new()),
            api_key_repo: Arc::new(MockApiKeyRepository::new()),
            dashboard_repo: Arc::new(dashboard_repo),
            webhook_repo: Arc::new(MockWebhookRepository::new()),
            db_pools: crate::config::DbPools::test_dummy(),
            jwt_secret: "test-secret".to_string(),
            redis_client: None,
        })
    }

    fn make_test_claims() -> SessionClaims {
        SessionClaims {
            sub: uuid::Uuid::new_v4().to_string(),
            name: "Test User".to_string(),
            org_id: uuid::Uuid::new_v4().to_string(),
            tenant_id: 1,
            role: "owner".to_string(),
            csrf: "test-csrf".to_string(),
            exp: (chrono::Utc::now() + chrono::Duration::hours(24)).timestamp() as usize,
            jti: String::new(),
        }
    }

    fn make_audit_entry(id: i64, action: &str) -> AuditLogEntry {
        AuditLogEntry {
            id,
            org_id: uuid::Uuid::new_v4(),
            actor_id: uuid::Uuid::new_v4(),
            action: action.to_string(),
            resource_type: "member".to_string(),
            resource_id: Some(uuid::Uuid::new_v4().to_string()),
            changes: Some(serde_json::json!({"email": "test@example.com"})),
            client_ip: None,
            created_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_list_audit_logs_returns_paginated_results() {
        let mut mock = MockDashboardRepository::new();
        let entry = make_audit_entry(1, "auth.login_success");

        mock.expect_list_audit_logs()
            .returning(move |_org_id, _filters, _offset, _limit| Ok((vec![entry.clone()], 1)));

        let state = make_test_state(mock);
        let claims = make_test_claims();

        let app = Router::new()
            .merge(audit_log_routes())
            .layer(axum::Extension(claims))
            .with_state(state);

        let req = Request::builder()
            .uri("/audit-logs?offset=0&limit=10")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["total"], 1);
        assert_eq!(json["offset"], 0);
        assert_eq!(json["limit"], 10);
        assert_eq!(json["data"].as_array().unwrap().len(), 1);
        assert_eq!(json["data"][0]["action"], "auth.login_success");
    }

    #[tokio::test]
    async fn test_list_audit_logs_empty_results() {
        let mut mock = MockDashboardRepository::new();
        mock.expect_list_audit_logs()
            .returning(|_, _, _, _| Ok((vec![], 0)));

        let state = make_test_state(mock);
        let claims = make_test_claims();

        let app = Router::new()
            .merge(audit_log_routes())
            .layer(axum::Extension(claims))
            .with_state(state);

        let req = Request::builder()
            .uri("/audit-logs")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["total"], 0);
        assert_eq!(json["data"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_list_audit_logs_default_pagination() {
        let mut mock = MockDashboardRepository::new();
        mock.expect_list_audit_logs()
            .withf(|_org_id, _filters, offset, limit| *offset == 0 && *limit == 50)
            .returning(|_, _, _, _| Ok((vec![], 0)));

        let state = make_test_state(mock);
        let claims = make_test_claims();

        let app = Router::new()
            .merge(audit_log_routes())
            .layer(axum::Extension(claims))
            .with_state(state);

        let req = Request::builder()
            .uri("/audit-logs")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_list_audit_logs_clamps_limit() {
        let mut mock = MockDashboardRepository::new();
        // limit=500 should be clamped to 100
        mock.expect_list_audit_logs()
            .withf(|_org_id, _filters, _offset, limit| *limit == 100)
            .returning(|_, _, _, _| Ok((vec![], 0)));

        let state = make_test_state(mock);
        let claims = make_test_claims();

        let app = Router::new()
            .merge(audit_log_routes())
            .layer(axum::Extension(claims))
            .with_state(state);

        let req = Request::builder()
            .uri("/audit-logs?limit=500")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_list_audit_logs_with_action_filter() {
        let mut mock = MockDashboardRepository::new();
        mock.expect_list_audit_logs()
            .withf(|_org_id, filters, _offset, _limit| {
                filters.action.as_deref() == Some("api_key.created")
            })
            .returning(|_, _, _, _| Ok((vec![], 0)));

        let state = make_test_state(mock);
        let claims = make_test_claims();

        let app = Router::new()
            .merge(audit_log_routes())
            .layer(axum::Extension(claims))
            .with_state(state);

        let req = Request::builder()
            .uri("/audit-logs?action=api_key.created")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_list_audit_logs_repo_error() {
        let mut mock = MockDashboardRepository::new();
        mock.expect_list_audit_logs().returning(|_, _, _, _| {
            Err(crate::repo::RepoError::Database(
                "connection lost".to_string(),
            ))
        });

        let state = make_test_state(mock);
        let claims = make_test_claims();

        let app = Router::new()
            .merge(audit_log_routes())
            .layer(axum::Extension(claims))
            .with_state(state);

        let req = Request::builder()
            .uri("/audit-logs")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
