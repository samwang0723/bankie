use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{self, Method},
    middleware::Next,
    response::Response,
};
use jsonwebtoken::{decode, DecodingKey, Validation};

use bankie_common::error::AppError;

use crate::models::auth::SessionClaims;
use crate::state::PortalState;

const SESSION_COOKIE_NAME: &str = "portal_session";

/// Middleware that validates JWT session cookies and CSRF tokens.
///
/// - Reads `portal_session` cookie from the request.
/// - Decodes and validates the JWT.
/// - For mutating methods (POST, PATCH, PUT, DELETE), validates CSRF token
///   from `X-CSRF-Token` header against the `csrf` claim in the JWT.
/// - Inserts `SessionClaims` into request extensions on success.
pub async fn session_auth(
    State(state): State<Arc<PortalState>>,
    mut request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let cookie_header = request
        .headers()
        .get(http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let token = extract_cookie(cookie_header, SESSION_COOKIE_NAME)
        .ok_or_else(|| AppError::Unauthorized("Missing session cookie".to_string()))?;

    let claims = decode::<SessionClaims>(
        token,
        &DecodingKey::from_secret(state.jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| AppError::Unauthorized("Invalid or expired session".to_string()))?
    .claims;

    // CSRF validation for mutating requests
    let method = request.method().clone();
    if matches!(
        method,
        Method::POST | Method::PATCH | Method::PUT | Method::DELETE
    ) {
        let csrf_header = request
            .headers()
            .get("x-csrf-token")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if csrf_header.is_empty() || csrf_header != claims.csrf {
            return Err(AppError::Forbidden("Invalid CSRF token".to_string()));
        }
    }

    request.extensions_mut().insert(claims);
    Ok(next.run(request).await)
}

/// Extract a named cookie value from a Cookie header string.
fn extract_cookie<'a>(header: &'a str, name: &str) -> Option<&'a str> {
    header.split(';').find_map(|pair| {
        let pair = pair.trim();
        if let Some(value) = pair.strip_prefix(name) {
            value.strip_prefix('=')
        } else {
            None
        }
    })
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
    use jsonwebtoken::{encode, EncodingKey, Header};
    use tower::ServiceExt;

    use crate::repo::api_key::MockApiKeyRepository;
    use crate::repo::dashboard::MockDashboardRepository;
    use crate::repo::member::MockMemberRepository;
    use crate::repo::org::MockOrgRepository;
    use crate::repo::webhook::MockWebhookRepository;

    fn test_state() -> Arc<PortalState> {
        Arc::new(PortalState {
            org_repo: Arc::new(MockOrgRepository::new()),
            member_repo: Arc::new(MockMemberRepository::new()),
            api_key_repo: Arc::new(MockApiKeyRepository::new()),
            dashboard_repo: Arc::new(MockDashboardRepository::new()),
            webhook_repo: Arc::new(MockWebhookRepository::new()),
            jwt_secret: "test-secret-key-at-least-32-chars-long!!".to_string(),
            redis_client: None,
        })
    }

    fn make_jwt(secret: &str, claims: &SessionClaims) -> String {
        encode(
            &Header::default(),
            claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap()
    }

    fn test_claims(csrf: &str) -> SessionClaims {
        SessionClaims {
            sub: uuid::Uuid::new_v4().to_string(),
            name: "Test User".to_string(),
            org_id: uuid::Uuid::new_v4().to_string(),
            tenant_id: 1,
            role: "owner".to_string(),
            csrf: csrf.to_string(),
            exp: (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp() as usize,
        }
    }

    async fn dummy_handler() -> &'static str {
        "ok"
    }

    fn test_app(state: Arc<PortalState>) -> Router {
        Router::new()
            .route("/test", get(dummy_handler).post(dummy_handler))
            .route_layer(axum_mw::from_fn_with_state(
                Arc::clone(&state),
                session_auth,
            ))
            .with_state(state)
    }

    #[tokio::test]
    async fn test_missing_cookie_returns_401() {
        let state = test_state();
        let app = test_app(state);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_invalid_jwt_returns_401() {
        let state = test_state();
        let app = test_app(state);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .header("Cookie", "portal_session=invalid-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_valid_jwt_get_succeeds_without_csrf() {
        let state = test_state();
        let claims = test_claims("csrf-token-123");
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = test_app(Arc::clone(&state));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .header("Cookie", format!("portal_session={jwt}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_post_without_csrf_returns_403() {
        let state = test_state();
        let claims = test_claims("csrf-token-123");
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = test_app(Arc::clone(&state));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/test")
                    .header("Cookie", format!("portal_session={jwt}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_post_with_wrong_csrf_returns_403() {
        let state = test_state();
        let claims = test_claims("correct-csrf");
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = test_app(Arc::clone(&state));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/test")
                    .header("Cookie", format!("portal_session={jwt}"))
                    .header("X-CSRF-Token", "wrong-csrf")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_post_with_correct_csrf_succeeds() {
        let state = test_state();
        let csrf = "correct-csrf-token";
        let claims = test_claims(csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = test_app(Arc::clone(&state));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/test")
                    .header("Cookie", format!("portal_session={jwt}"))
                    .header("X-CSRF-Token", csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn test_extract_cookie_found() {
        let header = "portal_session=abc123; other=xyz";
        assert_eq!(extract_cookie(header, "portal_session"), Some("abc123"));
    }

    #[test]
    fn test_extract_cookie_not_found() {
        let header = "other=xyz; another=123";
        assert_eq!(extract_cookie(header, "portal_session"), None);
    }

    #[test]
    fn test_extract_cookie_empty() {
        assert_eq!(extract_cookie("", "portal_session"), None);
    }
}
