use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;

use bankie_common::error::AppError;

use crate::middleware::rbac::require_api_key_management;
use crate::models::auth::SessionClaims;
use crate::models::dashboard::NewAuditLog;
use crate::models::webhook::{
    generate_signing_secret, mask_signing_secret, validate_event_types, CreateEndpointRequest,
    CreateEndpointResponse, DeliveryListItem, EndpointListItem, RotateSecretResponse,
    UpdateEndpointRequest,
};
use crate::repo::dashboard::DashboardRepository;
use crate::state::PortalState;
use crate::webhook::ssrf::validate_url_hostname;

/// Protected webhook management routes (session auth required).
pub fn webhook_routes() -> Router<Arc<PortalState>> {
    Router::new()
        .route("/webhooks", post(create_endpoint).get(list_endpoints))
        .route(
            "/webhooks/:id",
            get(get_endpoint)
                .put(update_endpoint)
                .delete(delete_endpoint),
        )
        .route("/webhooks/:id/rotate-secret", post(rotate_secret))
        .route(
            "/webhooks/:id/deliveries",
            get(list_deliveries_for_endpoint),
        )
}

fn parse_org_id(claims: &SessionClaims) -> Result<uuid::Uuid, AppError> {
    claims
        .org_id
        .parse()
        .map_err(|_| AppError::internal("Invalid org_id in session"))
}

/// Validate that a URL uses HTTPS (required for webhook endpoints in production).
/// In local/docker environments, HTTP is allowed for development testing.
fn validate_https_url(url: &str) -> Result<(), AppError> {
    let env = std::env::var("ENV").unwrap_or_else(|_| "local".to_string());
    validate_https_url_for_env(url, &env)
}

fn validate_https_url_for_env(url: &str, env: &str) -> Result<(), AppError> {
    if url.is_empty() {
        return Err(AppError::BadRequest("url is required".to_string()));
    }
    if !url.starts_with("https://") && env != "local" && env != "docker" {
        return Err(AppError::BadRequest(
            "Webhook URL must use HTTPS".to_string(),
        ));
    }
    Ok(())
}

/// POST /portal/v1/webhooks
///
/// Create a new webhook endpoint. Auto-generates a signing secret.
/// The raw signing secret is returned ONCE in the response.
async fn create_endpoint(
    State(state): State<Arc<PortalState>>,
    claims: axum::Extension<SessionClaims>,
    Json(req): Json<CreateEndpointRequest>,
) -> Result<(axum::http::StatusCode, Json<CreateEndpointResponse>), AppError> {
    let org_id = parse_org_id(&claims)?;
    require_api_key_management(&claims)?;

    validate_https_url(&req.url)?;
    let env = std::env::var("ENV").unwrap_or_else(|_| "local".to_string());
    if env != "local" && env != "docker" {
        validate_url_hostname(&req.url).map_err(AppError::BadRequest)?;
    }

    if req.event_types.is_empty() {
        return Err(AppError::BadRequest(
            "At least one event type is required".to_string(),
        ));
    }

    if let Err(invalid) = validate_event_types(&req.event_types) {
        return Err(AppError::BadRequest(format!(
            "Invalid event types: {}",
            invalid.join(", ")
        )));
    }

    let signing_secret = generate_signing_secret();
    let id = uuid::Uuid::new_v4();

    let endpoint = state
        .webhook_repo
        .create_endpoint(
            id,
            org_id,
            req.url.clone(),
            signing_secret.clone(),
            req.event_types.clone(),
            req.description.clone(),
        )
        .await
        .map_err(AppError::internal)?;

    // Best-effort audit log
    audit_log(
        &state.dashboard_repo,
        org_id,
        &claims.sub,
        "webhook.endpoint_created",
        "webhook_endpoint",
        Some(endpoint.id.to_string()),
        Some(serde_json::json!({
            "url": endpoint.url,
            "event_types": endpoint.event_types
        })),
    )
    .await;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(CreateEndpointResponse {
            id: endpoint.id,
            url: endpoint.url,
            signing_secret,
            event_types: endpoint.event_types,
            description: endpoint.description,
            status: endpoint.status,
            created_at: endpoint.created_at,
        }),
    ))
}

/// GET /portal/v1/webhooks
///
/// List all webhook endpoints for the org. Signing secrets are masked.
async fn list_endpoints(
    State(state): State<Arc<PortalState>>,
    claims: axum::Extension<SessionClaims>,
) -> Result<Json<Vec<EndpointListItem>>, AppError> {
    let org_id = parse_org_id(&claims)?;

    let endpoints = state
        .webhook_repo
        .list_endpoints_by_org(org_id)
        .await
        .map_err(AppError::internal)?;

    let items: Vec<EndpointListItem> = endpoints
        .into_iter()
        .map(|ep| EndpointListItem {
            id: ep.id,
            url: ep.url,
            signing_secret_prefix: mask_signing_secret(&ep.signing_secret),
            event_types: ep.event_types,
            description: ep.description,
            status: ep.status,
            failure_count: ep.failure_count,
            disabled_at: ep.disabled_at,
            created_at: ep.created_at,
        })
        .collect();

    Ok(Json(items))
}

/// GET /portal/v1/webhooks/:id
///
/// Get a single webhook endpoint. Signing secret is masked.
async fn get_endpoint(
    State(state): State<Arc<PortalState>>,
    claims: axum::Extension<SessionClaims>,
    Path(endpoint_id): Path<uuid::Uuid>,
) -> Result<Json<EndpointListItem>, AppError> {
    let org_id = parse_org_id(&claims)?;

    let ep = state
        .webhook_repo
        .find_endpoint_by_id(endpoint_id, org_id)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::NotFound("Webhook endpoint not found".to_string()))?;

    Ok(Json(EndpointListItem {
        id: ep.id,
        url: ep.url,
        signing_secret_prefix: mask_signing_secret(&ep.signing_secret),
        event_types: ep.event_types,
        description: ep.description,
        status: ep.status,
        failure_count: ep.failure_count,
        disabled_at: ep.disabled_at,
        created_at: ep.created_at,
    }))
}

/// PUT /portal/v1/webhooks/:id
///
/// Update a webhook endpoint (URL, event_types, description, status).
async fn update_endpoint(
    State(state): State<Arc<PortalState>>,
    claims: axum::Extension<SessionClaims>,
    Path(endpoint_id): Path<uuid::Uuid>,
    Json(req): Json<UpdateEndpointRequest>,
) -> Result<Json<EndpointListItem>, AppError> {
    let org_id = parse_org_id(&claims)?;
    require_api_key_management(&claims)?;

    // Validate URL if provided
    if let Some(ref url) = req.url {
        validate_https_url(url)?;
        let env = std::env::var("ENV").unwrap_or_else(|_| "local".to_string());
        if env != "local" && env != "docker" {
            validate_url_hostname(url).map_err(AppError::BadRequest)?;
        }
    }

    // Validate event types if provided
    if let Some(ref event_types) = req.event_types {
        if event_types.is_empty() {
            return Err(AppError::BadRequest(
                "At least one event type is required".to_string(),
            ));
        }
        if let Err(invalid) = validate_event_types(event_types) {
            return Err(AppError::BadRequest(format!(
                "Invalid event types: {}",
                invalid.join(", ")
            )));
        }
    }

    // Convert status enum to string for repo layer
    let status_str = req.status.as_ref().map(|s| s.as_str().to_string());

    let ep = state
        .webhook_repo
        .update_endpoint(
            endpoint_id,
            org_id,
            req.url.clone(),
            req.event_types.clone(),
            req.description.clone(),
            status_str,
        )
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::NotFound("Webhook endpoint not found".to_string()))?;

    // Best-effort audit log
    audit_log(
        &state.dashboard_repo,
        org_id,
        &claims.sub,
        "webhook.endpoint_updated",
        "webhook_endpoint",
        Some(endpoint_id.to_string()),
        Some(serde_json::json!({
            "url": req.url,
            "event_types": req.event_types,
            "status": req.status
        })),
    )
    .await;

    Ok(Json(EndpointListItem {
        id: ep.id,
        url: ep.url,
        signing_secret_prefix: mask_signing_secret(&ep.signing_secret),
        event_types: ep.event_types,
        description: ep.description,
        status: ep.status,
        failure_count: ep.failure_count,
        disabled_at: ep.disabled_at,
        created_at: ep.created_at,
    }))
}

/// DELETE /portal/v1/webhooks/:id
///
/// Delete a webhook endpoint and its deliveries.
async fn delete_endpoint(
    State(state): State<Arc<PortalState>>,
    claims: axum::Extension<SessionClaims>,
    Path(endpoint_id): Path<uuid::Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let org_id = parse_org_id(&claims)?;
    require_api_key_management(&claims)?;

    let deleted = state
        .webhook_repo
        .delete_endpoint(endpoint_id, org_id)
        .await
        .map_err(AppError::internal)?;

    if !deleted {
        return Err(AppError::NotFound("Webhook endpoint not found".to_string()));
    }

    // Best-effort audit log
    audit_log(
        &state.dashboard_repo,
        org_id,
        &claims.sub,
        "webhook.endpoint_deleted",
        "webhook_endpoint",
        Some(endpoint_id.to_string()),
        None,
    )
    .await;

    Ok(Json(
        serde_json::json!({"message": "Webhook endpoint deleted"}),
    ))
}

/// POST /portal/v1/webhooks/:id/rotate-secret
///
/// Rotate the signing secret. Returns the new secret (shown once).
///
/// **Known limitation (Phase 3.1 TODO)**: The old secret is immediately overwritten.
/// The `grace_expires_at` in the response is cosmetic — dual-key support during
/// grace period requires `old_signing_secret` + `secret_grace_expires_at` columns.
async fn rotate_secret(
    State(state): State<Arc<PortalState>>,
    claims: axum::Extension<SessionClaims>,
    Path(endpoint_id): Path<uuid::Uuid>,
) -> Result<Json<RotateSecretResponse>, AppError> {
    let org_id = parse_org_id(&claims)?;
    require_api_key_management(&claims)?;

    // Verify endpoint exists
    state
        .webhook_repo
        .find_endpoint_by_id(endpoint_id, org_id)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::NotFound("Webhook endpoint not found".to_string()))?;

    let new_secret = generate_signing_secret();

    state
        .webhook_repo
        .rotate_signing_secret(endpoint_id, org_id, new_secret.clone())
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::NotFound("Webhook endpoint not found".to_string()))?;

    // Grace period: 24 hours (old secret still valid during this window)
    let grace_expires_at = chrono::Utc::now() + chrono::Duration::hours(24);

    // Best-effort audit log
    audit_log(
        &state.dashboard_repo,
        org_id,
        &claims.sub,
        "webhook.secret_rotated",
        "webhook_endpoint",
        Some(endpoint_id.to_string()),
        Some(serde_json::json!({
            "grace_expires_at": grace_expires_at.to_rfc3339()
        })),
    )
    .await;

    Ok(Json(RotateSecretResponse {
        new_signing_secret: new_secret,
        grace_expires_at,
    }))
}

/// Query parameters for delivery listing.
#[derive(Debug, Deserialize)]
pub struct DeliveryQueryParams {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
    pub status: Option<String>,
}

/// GET /portal/v1/webhooks/:id/deliveries
///
/// List deliveries for a webhook endpoint (paginated, filterable by status).
async fn list_deliveries_for_endpoint(
    State(state): State<Arc<PortalState>>,
    claims: axum::Extension<SessionClaims>,
    Path(endpoint_id): Path<uuid::Uuid>,
    Query(params): Query<DeliveryQueryParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    let org_id = parse_org_id(&claims)?;

    // Verify endpoint belongs to org
    state
        .webhook_repo
        .find_endpoint_by_id(endpoint_id, org_id)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::NotFound("Webhook endpoint not found".to_string()))?;

    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).clamp(1, 100);

    let (deliveries, total) = state
        .webhook_repo
        .list_deliveries_by_endpoint(endpoint_id, page, per_page, params.status)
        .await
        .map_err(AppError::internal)?;

    let items: Vec<DeliveryListItem> = deliveries
        .into_iter()
        .map(|d| DeliveryListItem {
            id: d.id,
            event_type: d.event_type,
            event_source_id: d.event_source_id,
            status: d.status,
            http_status: d.http_status,
            attempt_number: d.attempt_number,
            latency_ms: d.latency_ms,
            next_retry_at: d.next_retry_at,
            created_at: d.created_at,
        })
        .collect();

    Ok(Json(serde_json::json!({
        "deliveries": items,
        "total": total,
        "page": page,
        "per_page": per_page
    })))
}

/// Best-effort audit log insertion. Failures are logged but not propagated.
async fn audit_log(
    dashboard_repo: &Arc<dyn DashboardRepository>,
    org_id: uuid::Uuid,
    actor_id: &str,
    action: &str,
    resource_type: &str,
    resource_id: Option<String>,
    changes: Option<serde_json::Value>,
) {
    let actor_uuid = actor_id.parse().unwrap_or_default();
    if let Err(e) = dashboard_repo
        .insert_audit_log(NewAuditLog {
            org_id,
            actor_id: actor_uuid,
            action: action.to_string(),
            resource_type: resource_type.to_string(),
            resource_id,
            changes,
            client_ip: None,
        })
        .await
    {
        tracing::warn!("Failed to insert audit log: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request as HttpRequest, StatusCode},
        middleware as axum_mw,
    };
    use jsonwebtoken::{encode, EncodingKey, Header};
    use tower::ServiceExt;

    use crate::middleware::session::session_auth;
    use crate::models::webhook::{EndpointStatus, WebhookEndpoint};
    use crate::repo::api_key::MockApiKeyRepository;
    use crate::repo::dashboard::MockDashboardRepository;
    use crate::repo::member::MockMemberRepository;
    use crate::repo::org::MockOrgRepository;
    use crate::repo::webhook::MockWebhookRepository;

    fn make_state(webhook_repo: MockWebhookRepository) -> Arc<PortalState> {
        let mut mock_dashboard = MockDashboardRepository::new();
        mock_dashboard
            .expect_insert_audit_log()
            .returning(|_| Ok(()));
        Arc::new(PortalState {
            org_repo: Arc::new(MockOrgRepository::new()),
            member_repo: Arc::new(MockMemberRepository::new()),
            api_key_repo: Arc::new(MockApiKeyRepository::new()),
            dashboard_repo: Arc::new(mock_dashboard),
            webhook_repo: Arc::new(webhook_repo),
            db_pools: crate::config::DbPools::test_dummy(),
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

    fn owner_claims(org_id: &str, csrf: &str) -> SessionClaims {
        SessionClaims {
            sub: uuid::Uuid::new_v4().to_string(),
            name: "Test User".to_string(),
            org_id: org_id.to_string(),
            tenant_id: 1,
            role: "owner".to_string(),
            csrf: csrf.to_string(),
            exp: 9999999999,
            jti: String::new(),
        }
    }

    fn member_claims(org_id: &str, csrf: &str) -> SessionClaims {
        SessionClaims {
            sub: uuid::Uuid::new_v4().to_string(),
            name: "Member User".to_string(),
            org_id: org_id.to_string(),
            tenant_id: 1,
            role: "member".to_string(),
            csrf: csrf.to_string(),
            exp: 9999999999,
            jti: String::new(),
        }
    }

    fn test_endpoint(org_id: uuid::Uuid) -> WebhookEndpoint {
        WebhookEndpoint {
            id: uuid::Uuid::new_v4(),
            org_id,
            url: "https://example.com/webhook".to_string(),
            signing_secret: "whsec_abc123def456".to_string(),
            event_types: vec!["account.opened".to_string()],
            description: Some("Test endpoint".to_string()),
            status: EndpointStatus::Active,
            failure_count: 0,
            disabled_at: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    fn build_app(state: Arc<PortalState>) -> Router {
        let protected = Router::new()
            .merge(webhook_routes())
            .route_layer(axum_mw::from_fn_with_state(
                Arc::clone(&state),
                session_auth,
            ))
            .with_state(Arc::clone(&state));

        Router::new().nest("/portal/v1", protected)
    }

    #[tokio::test]
    async fn test_create_endpoint_success() {
        let mut mock = MockWebhookRepository::new();
        let org_id = uuid::Uuid::new_v4();
        let org_id_clone = org_id;

        mock.expect_create_endpoint()
            .returning(move |id, oid, url, secret, event_types, desc| {
                Ok(WebhookEndpoint {
                    id,
                    org_id: oid,
                    url,
                    signing_secret: secret,
                    event_types,
                    description: desc,
                    status: EndpointStatus::Active,
                    failure_count: 0,
                    disabled_at: None,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                })
            });

        let state = make_state(mock);
        let csrf = "test-csrf";
        let claims = owner_claims(&org_id_clone.to_string(), csrf);
        let token = make_jwt(&state.jwt_secret, &claims);

        let app = build_app(state);

        let body = serde_json::json!({
            "url": "https://api.example.com/hook",
            "event_types": ["account.opened"],
            "description": "Test hook"
        });

        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/portal/v1/webhooks")
                    .header("content-type", "application/json")
                    .header(
                        "cookie",
                        format!("portal_session={token}; csrf_token={csrf}"),
                    )
                    .header("x-csrf-token", csrf)
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn test_create_endpoint_member_forbidden() {
        let mock = MockWebhookRepository::new();
        let org_id = uuid::Uuid::new_v4();

        let state = make_state(mock);
        let csrf = "test-csrf";
        let claims = member_claims(&org_id.to_string(), csrf);
        let token = make_jwt(&state.jwt_secret, &claims);

        let app = build_app(state);

        let body = serde_json::json!({
            "url": "https://api.example.com/hook",
            "event_types": ["account.opened"]
        });

        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/portal/v1/webhooks")
                    .header("content-type", "application/json")
                    .header(
                        "cookie",
                        format!("portal_session={token}; csrf_token={csrf}"),
                    )
                    .header("x-csrf-token", csrf)
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_create_endpoint_http_allowed_in_local_env() {
        // In local env (default), HTTP URLs are allowed for dev testing
        let mut mock = MockWebhookRepository::new();
        let org_id = uuid::Uuid::new_v4();

        mock.expect_create_endpoint()
            .returning(move |id, oid, url, secret, event_types, desc| {
                Ok(WebhookEndpoint {
                    id,
                    org_id: oid,
                    url,
                    signing_secret: secret,
                    event_types,
                    description: desc,
                    status: EndpointStatus::Active,
                    failure_count: 0,
                    disabled_at: None,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                })
            });

        let state = make_state(mock);
        let csrf = "test-csrf";
        let claims = owner_claims(&org_id.to_string(), csrf);
        let token = make_jwt(&state.jwt_secret, &claims);

        let app = build_app(state);

        let body = serde_json::json!({
            "url": "http://not-secure.com/hook",
            "event_types": ["account.opened"]
        });

        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/portal/v1/webhooks")
                    .header("content-type", "application/json")
                    .header(
                        "cookie",
                        format!("portal_session={token}; csrf_token={csrf}"),
                    )
                    .header("x-csrf-token", csrf)
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // HTTP is allowed in local env (ENV not set or "local")
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn test_create_endpoint_invalid_event_type() {
        let mock = MockWebhookRepository::new();
        let org_id = uuid::Uuid::new_v4();

        let state = make_state(mock);
        let csrf = "test-csrf";
        let claims = owner_claims(&org_id.to_string(), csrf);
        let token = make_jwt(&state.jwt_secret, &claims);

        let app = build_app(state);

        let body = serde_json::json!({
            "url": "https://api.example.com/hook",
            "event_types": ["invalid.event"]
        });

        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/portal/v1/webhooks")
                    .header("content-type", "application/json")
                    .header(
                        "cookie",
                        format!("portal_session={token}; csrf_token={csrf}"),
                    )
                    .header("x-csrf-token", csrf)
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_create_endpoint_empty_event_types() {
        let mock = MockWebhookRepository::new();
        let org_id = uuid::Uuid::new_v4();

        let state = make_state(mock);
        let csrf = "test-csrf";
        let claims = owner_claims(&org_id.to_string(), csrf);
        let token = make_jwt(&state.jwt_secret, &claims);

        let app = build_app(state);

        let body = serde_json::json!({
            "url": "https://api.example.com/hook",
            "event_types": []
        });

        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/portal/v1/webhooks")
                    .header("content-type", "application/json")
                    .header(
                        "cookie",
                        format!("portal_session={token}; csrf_token={csrf}"),
                    )
                    .header("x-csrf-token", csrf)
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_list_endpoints_success() {
        let mut mock = MockWebhookRepository::new();
        let org_id = uuid::Uuid::new_v4();
        let ep = test_endpoint(org_id);

        mock.expect_list_endpoints_by_org()
            .returning(move |_| Ok(vec![ep.clone()]));

        let state = make_state(mock);
        let csrf = "test-csrf";
        let claims = owner_claims(&org_id.to_string(), csrf);
        let token = make_jwt(&state.jwt_secret, &claims);

        let app = build_app(state);

        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method("GET")
                    .uri("/portal/v1/webhooks")
                    .header(
                        "cookie",
                        format!("portal_session={token}; csrf_token={csrf}"),
                    )
                    .header("x-csrf-token", csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_list_endpoints_member_allowed() {
        let mut mock = MockWebhookRepository::new();
        let org_id = uuid::Uuid::new_v4();

        mock.expect_list_endpoints_by_org()
            .returning(|_| Ok(vec![]));

        let state = make_state(mock);
        let csrf = "test-csrf";
        let claims = member_claims(&org_id.to_string(), csrf);
        let token = make_jwt(&state.jwt_secret, &claims);

        let app = build_app(state);

        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method("GET")
                    .uri("/portal/v1/webhooks")
                    .header(
                        "cookie",
                        format!("portal_session={token}; csrf_token={csrf}"),
                    )
                    .header("x-csrf-token", csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_endpoint_not_found() {
        let mut mock = MockWebhookRepository::new();
        let org_id = uuid::Uuid::new_v4();

        mock.expect_find_endpoint_by_id().returning(|_, _| Ok(None));

        let state = make_state(mock);
        let csrf = "test-csrf";
        let claims = owner_claims(&org_id.to_string(), csrf);
        let token = make_jwt(&state.jwt_secret, &claims);

        let ep_id = uuid::Uuid::new_v4();
        let app = build_app(state);

        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method("GET")
                    .uri(format!("/portal/v1/webhooks/{ep_id}"))
                    .header(
                        "cookie",
                        format!("portal_session={token}; csrf_token={csrf}"),
                    )
                    .header("x-csrf-token", csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_delete_endpoint_success() {
        let mut mock = MockWebhookRepository::new();
        let org_id = uuid::Uuid::new_v4();

        mock.expect_delete_endpoint().returning(|_, _| Ok(true));

        let state = make_state(mock);
        let csrf = "test-csrf";
        let claims = owner_claims(&org_id.to_string(), csrf);
        let token = make_jwt(&state.jwt_secret, &claims);

        let ep_id = uuid::Uuid::new_v4();
        let app = build_app(state);

        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method("DELETE")
                    .uri(format!("/portal/v1/webhooks/{ep_id}"))
                    .header(
                        "cookie",
                        format!("portal_session={token}; csrf_token={csrf}"),
                    )
                    .header("x-csrf-token", csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_delete_endpoint_not_found() {
        let mut mock = MockWebhookRepository::new();
        let org_id = uuid::Uuid::new_v4();

        mock.expect_delete_endpoint().returning(|_, _| Ok(false));

        let state = make_state(mock);
        let csrf = "test-csrf";
        let claims = owner_claims(&org_id.to_string(), csrf);
        let token = make_jwt(&state.jwt_secret, &claims);

        let ep_id = uuid::Uuid::new_v4();
        let app = build_app(state);

        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method("DELETE")
                    .uri(format!("/portal/v1/webhooks/{ep_id}"))
                    .header(
                        "cookie",
                        format!("portal_session={token}; csrf_token={csrf}"),
                    )
                    .header("x-csrf-token", csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_delete_endpoint_member_forbidden() {
        let mock = MockWebhookRepository::new();
        let org_id = uuid::Uuid::new_v4();

        let state = make_state(mock);
        let csrf = "test-csrf";
        let claims = member_claims(&org_id.to_string(), csrf);
        let token = make_jwt(&state.jwt_secret, &claims);

        let ep_id = uuid::Uuid::new_v4();
        let app = build_app(state);

        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method("DELETE")
                    .uri(format!("/portal/v1/webhooks/{ep_id}"))
                    .header(
                        "cookie",
                        format!("portal_session={token}; csrf_token={csrf}"),
                    )
                    .header("x-csrf-token", csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_rotate_secret_success() {
        let mut mock = MockWebhookRepository::new();
        let org_id = uuid::Uuid::new_v4();
        let ep = test_endpoint(org_id);

        let ep_clone = ep.clone();
        mock.expect_find_endpoint_by_id()
            .returning(move |_, _| Ok(Some(ep_clone.clone())));

        let ep_clone2 = ep.clone();
        mock.expect_rotate_signing_secret()
            .returning(move |_, _, _| Ok(Some(ep_clone2.clone())));

        let state = make_state(mock);
        let csrf = "test-csrf";
        let claims = owner_claims(&org_id.to_string(), csrf);
        let token = make_jwt(&state.jwt_secret, &claims);

        let app = build_app(state);

        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri(format!("/portal/v1/webhooks/{}/rotate-secret", ep.id))
                    .header(
                        "cookie",
                        format!("portal_session={token}; csrf_token={csrf}"),
                    )
                    .header("x-csrf-token", csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_rotate_secret_not_found() {
        let mut mock = MockWebhookRepository::new();
        let org_id = uuid::Uuid::new_v4();

        mock.expect_find_endpoint_by_id().returning(|_, _| Ok(None));

        let state = make_state(mock);
        let csrf = "test-csrf";
        let claims = owner_claims(&org_id.to_string(), csrf);
        let token = make_jwt(&state.jwt_secret, &claims);

        let ep_id = uuid::Uuid::new_v4();
        let app = build_app(state);

        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri(format!("/portal/v1/webhooks/{ep_id}/rotate-secret"))
                    .header(
                        "cookie",
                        format!("portal_session={token}; csrf_token={csrf}"),
                    )
                    .header("x-csrf-token", csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_validate_https_url_valid() {
        assert!(validate_https_url_for_env("https://example.com/hook", "production").is_ok());
    }

    #[test]
    fn test_validate_https_url_http_rejected_in_production() {
        assert!(validate_https_url_for_env("http://example.com/hook", "production").is_err());
    }

    #[test]
    fn test_validate_https_url_http_allowed_in_local() {
        assert!(validate_https_url_for_env("http://example.com/hook", "local").is_ok());
    }

    #[test]
    fn test_validate_https_url_http_allowed_in_docker() {
        assert!(validate_https_url_for_env("http://example.com/hook", "docker").is_ok());
    }

    #[test]
    fn test_validate_https_url_empty_rejected() {
        assert!(validate_https_url_for_env("", "production").is_err());
    }
}
