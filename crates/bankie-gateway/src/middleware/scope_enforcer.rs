use std::sync::Arc;

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};

use bankie_common::error::AppError;

use crate::models::api_key::hash_key;
use crate::state::PortalState;

/// Middleware that validates API key bearer tokens and enforces scope-based access.
///
/// Reads `Authorization: Bearer bk_live_xxx` header, hashes the key, looks it up
/// in the database, and checks that the key's scopes include the required scope
/// for the target route.
pub async fn scope_enforcer(
    State(state): State<Arc<PortalState>>,
    mut request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let auth_header = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let raw_key = auth_header.strip_prefix("Bearer ").ok_or_else(|| {
        AppError::Unauthorized("Missing or invalid Authorization header".to_string())
    })?;

    if !raw_key.starts_with("bk_live_") {
        return Err(AppError::Unauthorized("Invalid API key format".to_string()));
    }

    let key_hash = hash_key(raw_key);
    let api_key = state
        .api_key_repo
        .find_valid_by_hash(key_hash)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::Unauthorized("Invalid or expired API key".to_string()))?;

    let required_scope = route_to_scope(request.method().as_str(), request.uri().path());

    if let Some(scope) = required_scope {
        if !api_key.scopes.contains(&scope.to_string()) {
            return Err(AppError::Forbidden(format!(
                "API key missing required scope: {scope}"
            )));
        }
    }

    // Store tenant_id from the API key for downstream handlers
    request.extensions_mut().insert(api_key.tenant_id);
    Ok(next.run(request).await)
}

/// Map a (method, path) to the required scope. Returns `None` if no scope required.
fn route_to_scope(method: &str, path: &str) -> Option<&'static str> {
    // Normalize path by removing trailing slash
    let path = path.trim_end_matches('/');

    // Match route patterns to scopes
    match (method, path) {
        // Bank account routes
        (m, p) if p.starts_with("/v1/bank_account") => {
            if matches!(m, "POST" | "PATCH" | "PUT" | "DELETE") {
                Some("accounts:write")
            } else {
                Some("accounts:read")
            }
        }
        // Account listing
        (_, p) if p.starts_with("/v1/accounts") => Some("accounts:read"),
        // User routes
        (_, p) if p.starts_with("/v1/user") => Some("accounts:read"),
        // Ledger routes
        (_, p) if p.starts_with("/v1/ledger") => Some("ledgers:read"),
        // Transaction routes
        (_, p) if p.starts_with("/v1/transaction") => Some("transactions:read"),
        // House account routes
        (m, p) if p.starts_with("/v1/house_account") => {
            if matches!(m, "POST" | "PATCH" | "PUT" | "DELETE") {
                Some("house_accounts:write")
            } else {
                Some("house_accounts:read")
            }
        }
        // Report routes
        (_, p) if p.starts_with("/v1/report") => Some("transactions:read"),
        // Health/ready (no scope needed)
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request as HttpRequest, StatusCode},
        middleware as axum_mw,
        routing::get,
        Router,
    };
    use tower::ServiceExt;

    use crate::models::api_key::{ApiKey, KeyStatus};
    use crate::repo::api_key::MockApiKeyRepository;
    use crate::repo::member::MockMemberRepository;
    use crate::repo::org::MockOrgRepository;

    fn make_state(mock_repo: MockApiKeyRepository) -> Arc<PortalState> {
        Arc::new(PortalState {
            org_repo: Arc::new(MockOrgRepository::new()),
            member_repo: Arc::new(MockMemberRepository::new()),
            api_key_repo: Arc::new(mock_repo),
            jwt_secret: "test-secret".to_string(),
        })
    }

    fn test_api_key(scopes: Vec<String>) -> ApiKey {
        ApiKey {
            id: uuid::Uuid::new_v4(),
            org_id: uuid::Uuid::new_v4(),
            tenant_id: 1,
            name: "test-key".to_string(),
            key_prefix: "bk_live_test1234".to_string(),
            key_hash: "hash".to_string(),
            scopes,
            status: KeyStatus::Active,
            grace_expires_at: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    async fn dummy_handler() -> &'static str {
        "ok"
    }

    fn test_app(state: Arc<PortalState>) -> Router {
        Router::new()
            .route("/v1/bank_account/123", get(dummy_handler))
            .route_layer(axum_mw::from_fn_with_state(
                Arc::clone(&state),
                scope_enforcer,
            ))
            .with_state(state)
    }

    #[tokio::test]
    async fn test_missing_auth_header_returns_401() {
        let state = make_state(MockApiKeyRepository::new());
        let app = test_app(state);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/v1/bank_account/123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_invalid_key_format_returns_401() {
        let state = make_state(MockApiKeyRepository::new());
        let app = test_app(state);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/v1/bank_account/123")
                    .header("Authorization", "Bearer invalid_key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_key_not_found_returns_401() {
        let mut mock_repo = MockApiKeyRepository::new();
        mock_repo
            .expect_find_valid_by_hash()
            .returning(|_| Ok(None));

        let state = make_state(mock_repo);
        let app = test_app(state);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/v1/bank_account/123")
                    .header(
                        "Authorization",
                        "Bearer bk_live_testkey12345678901234567890123456789012345678",
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_key_missing_scope_returns_403() {
        let mut mock_repo = MockApiKeyRepository::new();
        mock_repo
            .expect_find_valid_by_hash()
            .returning(|_| Ok(Some(test_api_key(vec!["ledgers:read".to_string()]))));

        let state = make_state(mock_repo);
        let app = test_app(state);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/v1/bank_account/123")
                    .header(
                        "Authorization",
                        "Bearer bk_live_testkey12345678901234567890123456789012345678",
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_key_with_correct_scope_succeeds() {
        let mut mock_repo = MockApiKeyRepository::new();
        mock_repo
            .expect_find_valid_by_hash()
            .returning(|_| Ok(Some(test_api_key(vec!["accounts:read".to_string()]))));

        let state = make_state(mock_repo);
        let app = test_app(state);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/v1/bank_account/123")
                    .header(
                        "Authorization",
                        "Bearer bk_live_testkey12345678901234567890123456789012345678",
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn test_route_to_scope_bank_account_read() {
        assert_eq!(
            route_to_scope("GET", "/v1/bank_account/123"),
            Some("accounts:read")
        );
    }

    #[test]
    fn test_route_to_scope_bank_account_write() {
        assert_eq!(
            route_to_scope("POST", "/v1/bank_account"),
            Some("accounts:write")
        );
    }

    #[test]
    fn test_route_to_scope_ledger() {
        assert_eq!(
            route_to_scope("GET", "/v1/ledger/abc"),
            Some("ledgers:read")
        );
    }

    #[test]
    fn test_route_to_scope_transaction() {
        assert_eq!(
            route_to_scope("GET", "/v1/transaction"),
            Some("transactions:read")
        );
    }

    #[test]
    fn test_route_to_scope_house_account_read() {
        assert_eq!(
            route_to_scope("GET", "/v1/house_account"),
            Some("house_accounts:read")
        );
    }

    #[test]
    fn test_route_to_scope_house_account_write() {
        assert_eq!(
            route_to_scope("POST", "/v1/house_account"),
            Some("house_accounts:write")
        );
    }

    #[test]
    fn test_route_to_scope_health_no_scope() {
        assert_eq!(route_to_scope("GET", "/health"), None);
    }

    #[test]
    fn test_route_to_scope_report() {
        assert_eq!(
            route_to_scope("GET", "/v1/report/settlement"),
            Some("transactions:read")
        );
    }
}
