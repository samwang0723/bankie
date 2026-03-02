use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Path, Query},
    http::{Request, Response, StatusCode, Uri},
    routing::get,
    Router,
};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use serde::Deserialize;
use tracing::{error, info};

use crate::config::SETTINGS;
use crate::models::auth::SessionClaims;
use crate::state::PortalState;

/// Protected data proxy routes (session auth required).
/// Proxies GET requests to bankie-core using the session's tenant_id.
pub fn data_proxy_routes() -> Router<Arc<PortalState>> {
    Router::new()
        .route("/data/accounts", get(list_accounts))
        .route("/data/accounts/{id}", get(get_account))
        .route("/data/accounts/{id}/sub-accounts", get(get_sub_accounts))
        .route("/data/accounts/{id}/balance-history", get(balance_history))
        .route("/data/transactions", get(list_transactions))
        .route("/data/ledger/{id}", get(get_ledger))
        .route("/data/reports/settlement", get(settlement_report))
}

/// Mint a short-lived JWT from session claims for proxying to bankie-core.
fn mint_portal_jwt(session: &SessionClaims, secret: &str) -> Result<String, StatusCode> {
    use chrono::Utc;
    use jsonwebtoken::{encode, EncodingKey, Header};

    let now = Utc::now().timestamp();

    let claims = crate::middleware::jwt_minter::Claims {
        iss: "bankie-gateway".to_string(),
        sub: format!("portal:{}", session.sub),
        aud: "service".to_string(),
        exp: (now + 60) as usize,
        iat: now as usize,
        scopes: vec!["portal:admin".to_string()],
        tenant_id: session.tenant_id,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| {
        error!("Failed to mint portal JWT: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })
}

/// Forward a GET request to bankie-core and return the response as-is.
async fn forward_get(
    session: &SessionClaims,
    core_path: &str,
) -> Result<Response<Body>, StatusCode> {
    let settings = SETTINGS.clone();
    let jwt_secret = &settings.jwt_secret;

    let token = mint_portal_jwt(session, jwt_secret)?;
    let upstream_url = format!("{}{}", settings.core_url, core_path);

    info!(upstream_url = %upstream_url, tenant_id = session.tenant_id, "Portal data proxy");

    let upstream_uri: Uri = upstream_url.parse().map_err(|e| {
        error!("Failed to parse upstream URL: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let req = Request::builder()
        .method("GET")
        .uri(upstream_uri)
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::empty())
        .map_err(|e| {
            error!("Failed to build upstream request: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let client = Client::builder(TokioExecutor::new()).build_http();
    let resp = client.request(req).await.map_err(|e| {
        error!("Upstream request failed: {}", e);
        StatusCode::BAD_GATEWAY
    })?;

    // Convert hyper response to axum response preserving status + headers + body
    let (parts, body) = resp.into_parts();
    Ok(Response::from_parts(parts, Body::new(body)))
}

// --- Query parameter structs ---

#[derive(Debug, Deserialize)]
struct PaginationParams {
    offset: Option<i64>,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct TransactionParams {
    bank_account_id: Option<String>,
    offset: Option<i64>,
    limit: Option<i64>,
    start_date: Option<String>,
    end_date: Option<String>,
    transaction_type: Option<String>,
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BalanceHistoryParams {
    start_date: Option<String>,
    end_date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SettlementParams {
    start_date: Option<String>,
    end_date: Option<String>,
    bank_account_id: Option<String>,
    currency: Option<String>,
}

// --- Helper to build query string ---

fn build_query(params: &[(&str, &Option<String>)]) -> String {
    let parts: Vec<String> = params
        .iter()
        .filter_map(|(key, val)| val.as_ref().map(|v| format!("{}={}", key, v)))
        .collect();
    if parts.is_empty() {
        String::new()
    } else {
        format!("?{}", parts.join("&"))
    }
}

// --- Route handlers ---

async fn list_accounts(
    claims: axum::Extension<SessionClaims>,
    Query(params): Query<PaginationParams>,
) -> Result<Response<Body>, StatusCode> {
    let offset = params.offset.map(|v| v.to_string());
    let limit = params.limit.map(|v| v.to_string());
    let qs = build_query(&[("offset", &offset), ("limit", &limit)]);
    forward_get(&claims, &format!("/v1/accounts{}", qs)).await
}

async fn get_account(
    claims: axum::Extension<SessionClaims>,
    Path(id): Path<String>,
) -> Result<Response<Body>, StatusCode> {
    forward_get(&claims, &format!("/v1/bank_account/{}", id)).await
}

async fn get_sub_accounts(
    claims: axum::Extension<SessionClaims>,
    Path(id): Path<String>,
) -> Result<Response<Body>, StatusCode> {
    forward_get(&claims, &format!("/v1/bank_account/{}/sub-accounts", id)).await
}

async fn balance_history(
    claims: axum::Extension<SessionClaims>,
    Path(id): Path<String>,
    Query(params): Query<BalanceHistoryParams>,
) -> Result<Response<Body>, StatusCode> {
    let qs = build_query(&[
        ("start_date", &params.start_date),
        ("end_date", &params.end_date),
    ]);
    forward_get(
        &claims,
        &format!("/v1/bank_account/{}/balance-history{}", id, qs),
    )
    .await
}

async fn list_transactions(
    claims: axum::Extension<SessionClaims>,
    Query(params): Query<TransactionParams>,
) -> Result<Response<Body>, StatusCode> {
    let offset = params.offset.map(|v| v.to_string());
    let limit = params.limit.map(|v| v.to_string());
    let qs = build_query(&[
        ("bank_account_id", &params.bank_account_id),
        ("offset", &offset),
        ("limit", &limit),
        ("start_date", &params.start_date),
        ("end_date", &params.end_date),
        ("transaction_type", &params.transaction_type),
        ("status", &params.status),
    ]);
    forward_get(&claims, &format!("/v1/transaction{}", qs)).await
}

async fn get_ledger(
    claims: axum::Extension<SessionClaims>,
    Path(id): Path<String>,
) -> Result<Response<Body>, StatusCode> {
    forward_get(&claims, &format!("/v1/ledger/{}", id)).await
}

async fn settlement_report(
    claims: axum::Extension<SessionClaims>,
    Query(params): Query<SettlementParams>,
) -> Result<Response<Body>, StatusCode> {
    let qs = build_query(&[
        ("start_date", &params.start_date),
        ("end_date", &params.end_date),
        ("bank_account_id", &params.bank_account_id),
        ("currency", &params.currency),
    ]);
    forward_get(&claims, &format!("/v1/report/settlement{}", qs)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- build_query tests ---

    #[test]
    fn test_build_query_empty() {
        let result = build_query(&[]);
        assert_eq!(result, "");
    }

    #[test]
    fn test_build_query_some_params() {
        let offset = Some("0".to_string());
        let limit = Some("10".to_string());
        let result = build_query(&[("offset", &offset), ("limit", &limit)]);
        assert_eq!(result, "?offset=0&limit=10");
    }

    #[test]
    fn test_build_query_all_none() {
        let none: Option<String> = None;
        let result = build_query(&[("offset", &none), ("limit", &none)]);
        assert_eq!(result, "");
    }

    #[test]
    fn test_build_query_mixed_some_none() {
        let none: Option<String> = None;
        let limit = Some("25".to_string());
        let result = build_query(&[("offset", &none), ("limit", &limit)]);
        assert_eq!(result, "?limit=25");
    }

    #[test]
    fn test_build_query_url_encodes_values() {
        let date = Some("2026-01-01".to_string());
        let result = build_query(&[("start_date", &date)]);
        assert_eq!(result, "?start_date=2026-01-01");
    }

    // --- mint_portal_jwt tests ---

    #[test]
    fn test_mint_portal_jwt_success() {
        let session = SessionClaims {
            sub: "user-123".to_string(),
            name: "Test User".to_string(),
            org_id: "org-456".to_string(),
            tenant_id: 42,
            role: "owner".to_string(),
            csrf: "csrf-token".to_string(),
            exp: 9999999999,
        };
        let secret = "test-secret-key-for-jwt";

        let result = mint_portal_jwt(&session, secret);
        assert!(result.is_ok(), "mint_portal_jwt should succeed");
        assert!(!result.unwrap().is_empty(), "JWT token should not be empty");
    }

    #[test]
    fn test_mint_portal_jwt_claims_match() {
        use jsonwebtoken::{decode, DecodingKey, Validation};

        let session = SessionClaims {
            sub: "user-123".to_string(),
            name: "Test User".to_string(),
            org_id: "org-456".to_string(),
            tenant_id: 42,
            role: "owner".to_string(),
            csrf: "csrf-token".to_string(),
            exp: 9999999999,
        };
        let secret = "test-secret-key-for-jwt";

        let token = mint_portal_jwt(&session, secret).unwrap();

        let mut validation = Validation::default();
        validation.set_audience(&["service"]);
        validation.set_issuer(&["bankie-gateway"]);

        let decoded = decode::<crate::middleware::jwt_minter::Claims>(
            &token,
            &DecodingKey::from_secret(secret.as_bytes()),
            &validation,
        )
        .unwrap();

        assert_eq!(decoded.claims.iss, "bankie-gateway");
        assert_eq!(decoded.claims.aud, "service");
        assert_eq!(decoded.claims.tenant_id, 42);
        assert_eq!(decoded.claims.sub, "portal:user-123");
        assert_eq!(decoded.claims.scopes, vec!["portal:admin"]);
    }

    #[test]
    fn test_mint_portal_jwt_short_lived() {
        use jsonwebtoken::{decode, DecodingKey, Validation};

        let session = SessionClaims {
            sub: "user-abc".to_string(),
            name: "Test User".to_string(),
            org_id: "org-def".to_string(),
            tenant_id: 7,
            role: "admin".to_string(),
            csrf: "c".to_string(),
            exp: 9999999999,
        };
        let secret = "test-secret";

        let token = mint_portal_jwt(&session, secret).unwrap();

        let mut validation = Validation::default();
        validation.set_audience(&["service"]);
        let decoded = decode::<crate::middleware::jwt_minter::Claims>(
            &token,
            &DecodingKey::from_secret(secret.as_bytes()),
            &validation,
        )
        .unwrap();

        let ttl = decoded.claims.exp - decoded.claims.iat;
        assert_eq!(ttl, 60, "Portal JWT should have 60s TTL");
    }
}
