use std::sync::Arc;

use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;

use bankie_common::error::AppError;

use crate::middleware::api_logger::redact_ip;
use crate::models::auth::SessionClaims;
use crate::models::webhook::ApiLogFilters;
use crate::state::PortalState;

/// Protected log viewer routes (session auth required).
pub fn logs_routes() -> Router<Arc<PortalState>> {
    Router::new().route("/logs", get(list_logs))
}

/// Query parameters for the API logs viewer.
#[derive(Debug, Deserialize)]
pub struct LogQueryParams {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
    pub method: Option<String>,
    pub status_code: Option<i32>,
    pub path: Option<String>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
}

/// GET /portal/v1/logs
///
/// Returns paginated API logs for the current tenant, with optional filters.
/// Supported filters: method, status_code, path (prefix match), from/to (time range).
/// IP addresses in results are redacted to /24 (IPv4) or /48 (IPv6).
async fn list_logs(
    state: State<Arc<PortalState>>,
    claims: axum::Extension<SessionClaims>,
    Query(params): Query<LogQueryParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    let tenant_id = claims.tenant_id;

    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(50).clamp(1, 100);

    let filters = ApiLogFilters {
        method: params.method,
        status_code: params.status_code,
        path: params.path,
        from: params.from,
        to: params.to,
    };

    let (logs, total) = state
        .webhook_repo
        .list_api_logs(tenant_id, filters, page, per_page)
        .await
        .map_err(AppError::internal)?;

    // Redact IPs in response
    let redacted_logs: Vec<serde_json::Value> = logs
        .into_iter()
        .map(|entry| {
            serde_json::json!({
                "id": entry.id,
                "method": entry.method,
                "path": entry.path,
                "status_code": entry.status_code,
                "latency_ms": entry.latency_ms,
                "client_ip": entry.client_ip.as_deref().map(redact_ip),
                "created_at": entry.created_at,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "logs": redacted_logs,
        "total": total,
        "page": page,
        "per_page": per_page,
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
    use crate::models::webhook::{ApiLogEntry, ApiLogFilters};
    use crate::repo::api_key::MockApiKeyRepository;
    use crate::repo::dashboard::MockDashboardRepository;
    use crate::repo::member::MockMemberRepository;
    use crate::repo::org::MockOrgRepository;
    use crate::repo::webhook::MockWebhookRepository;
    use crate::state::PortalState;

    fn make_test_state(webhook_repo: MockWebhookRepository) -> Arc<PortalState> {
        Arc::new(PortalState {
            org_repo: Arc::new(MockOrgRepository::new()),
            member_repo: Arc::new(MockMemberRepository::new()),
            api_key_repo: Arc::new(MockApiKeyRepository::new()),
            dashboard_repo: Arc::new(MockDashboardRepository::new()),
            webhook_repo: Arc::new(webhook_repo),
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

    fn make_log_entry(id: i64, method: &str, path: &str, status: i32) -> ApiLogEntry {
        ApiLogEntry {
            id,
            api_key_id: Some(uuid::Uuid::new_v4()),
            tenant_id: Some(1),
            method: method.to_string(),
            path: path.to_string(),
            status_code: status,
            latency_ms: Some(42),
            client_ip: Some("192.168.1.100".to_string()),
            created_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_list_logs_returns_paginated_results() {
        let mut mock = MockWebhookRepository::new();
        let entry = make_log_entry(1, "GET", "/v1/accounts", 200);

        mock.expect_list_api_logs()
            .returning(move |_tid, _f, _p, _pp| Ok((vec![entry.clone()], 1)));

        let state = make_test_state(mock);
        let claims = make_test_claims();

        let app = Router::new()
            .merge(logs_routes())
            .layer(axum::Extension(claims))
            .with_state(state);

        let req = Request::builder()
            .uri("/logs?page=1&per_page=10")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["total"], 1);
        assert_eq!(json["page"], 1);
        assert_eq!(json["per_page"], 10);
        assert_eq!(json["logs"].as_array().unwrap().len(), 1);
        // IP should be redacted to /24
        assert_eq!(json["logs"][0]["client_ip"], "192.168.1.0");
    }

    #[tokio::test]
    async fn test_list_logs_empty_results() {
        let mut mock = MockWebhookRepository::new();
        mock.expect_list_api_logs()
            .returning(|_, _, _, _| Ok((vec![], 0)));

        let state = make_test_state(mock);
        let claims = make_test_claims();

        let app = Router::new()
            .merge(logs_routes())
            .layer(axum::Extension(claims))
            .with_state(state);

        let req = Request::builder().uri("/logs").body(Body::empty()).unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["total"], 0);
        assert_eq!(json["logs"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_list_logs_defaults() {
        let mut mock = MockWebhookRepository::new();
        mock.expect_list_api_logs()
            .withf(|tid, _f, page, per_page| *tid == 1 && *page == 1 && *per_page == 50)
            .returning(|_, _, _, _| Ok((vec![], 0)));

        let state = make_test_state(mock);
        let claims = make_test_claims();

        let app = Router::new()
            .merge(logs_routes())
            .layer(axum::Extension(claims))
            .with_state(state);

        let req = Request::builder().uri("/logs").body(Body::empty()).unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_list_logs_clamps_per_page() {
        let mut mock = MockWebhookRepository::new();
        // per_page=500 should be clamped to 100
        mock.expect_list_api_logs()
            .withf(|_tid, _f, _page, per_page| *per_page == 100)
            .returning(|_, _, _, _| Ok((vec![], 0)));

        let state = make_test_state(mock);
        let claims = make_test_claims();

        let app = Router::new()
            .merge(logs_routes())
            .layer(axum::Extension(claims))
            .with_state(state);

        let req = Request::builder()
            .uri("/logs?per_page=500")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_list_logs_redacts_ip_in_response() {
        let mut mock = MockWebhookRepository::new();
        let entry = ApiLogEntry {
            id: 1,
            api_key_id: Some(uuid::Uuid::new_v4()),
            tenant_id: Some(1),
            method: "POST".to_string(),
            path: "/v1/bank_account".to_string(),
            status_code: 201,
            latency_ms: Some(15),
            client_ip: Some("10.20.30.40".to_string()),
            created_at: chrono::Utc::now(),
        };

        mock.expect_list_api_logs()
            .returning(move |_, _, _, _| Ok((vec![entry.clone()], 1)));

        let state = make_test_state(mock);
        let claims = make_test_claims();

        let app = Router::new()
            .merge(logs_routes())
            .layer(axum::Extension(claims))
            .with_state(state);

        let req = Request::builder().uri("/logs").body(Body::empty()).unwrap();

        let resp = app.oneshot(req).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // IP should be redacted from 10.20.30.40 to 10.20.30.0
        assert_eq!(json["logs"][0]["client_ip"], "10.20.30.0");
    }

    #[tokio::test]
    async fn test_list_logs_null_ip() {
        let mut mock = MockWebhookRepository::new();
        let entry = ApiLogEntry {
            id: 1,
            api_key_id: None,
            tenant_id: Some(1),
            method: "GET".to_string(),
            path: "/v1/accounts".to_string(),
            status_code: 200,
            latency_ms: None,
            client_ip: None,
            created_at: chrono::Utc::now(),
        };

        mock.expect_list_api_logs()
            .returning(move |_, _, _, _| Ok((vec![entry.clone()], 1)));

        let state = make_test_state(mock);
        let claims = make_test_claims();

        let app = Router::new()
            .merge(logs_routes())
            .layer(axum::Extension(claims))
            .with_state(state);

        let req = Request::builder().uri("/logs").body(Body::empty()).unwrap();

        let resp = app.oneshot(req).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert!(json["logs"][0]["client_ip"].is_null());
    }

    #[tokio::test]
    async fn test_list_logs_repo_error() {
        let mut mock = MockWebhookRepository::new();
        mock.expect_list_api_logs().returning(|_, _, _, _| {
            Err(crate::repo::RepoError::Database(
                "connection lost".to_string(),
            ))
        });

        let state = make_test_state(mock);
        let claims = make_test_claims();

        let app = Router::new()
            .merge(logs_routes())
            .layer(axum::Extension(claims))
            .with_state(state);

        let req = Request::builder().uri("/logs").body(Body::empty()).unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    // === LogQueryParams tests ===

    #[test]
    fn test_log_query_params_defaults() {
        let params = LogQueryParams {
            page: None,
            per_page: None,
            method: None,
            status_code: None,
            path: None,
            from: None,
            to: None,
        };
        assert!(params.page.is_none());
        assert!(params.per_page.is_none());
    }

    #[test]
    fn test_api_log_filters_from_params() {
        let params = LogQueryParams {
            page: Some(2),
            per_page: Some(25),
            method: Some("GET".to_string()),
            status_code: Some(200),
            path: Some("/v1/".to_string()),
            from: None,
            to: None,
        };
        let filters = ApiLogFilters {
            method: params.method,
            status_code: params.status_code,
            path: params.path,
            from: params.from,
            to: params.to,
        };
        assert_eq!(filters.method.as_deref(), Some("GET"));
        assert_eq!(filters.status_code, Some(200));
        assert_eq!(filters.path.as_deref(), Some("/v1/"));
    }
}
