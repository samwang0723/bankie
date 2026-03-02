use std::sync::Arc;

use axum::{
    extract::{Path, State},
    routing::{delete, post},
    Json, Router,
};
use chrono::{Duration, Utc};

use bankie_common::error::AppError;

use crate::middleware::rbac::require_api_key_management;
use crate::models::api_key::{
    generate_raw_key, hash_key, key_prefix, validate_scopes, CreateKeyRequest, CreateKeyResponse,
    KeyListItem, KeyStatus, RotateKeyResponse,
};
use crate::models::auth::SessionClaims;
use crate::models::dashboard::NewAuditLog;
use crate::repo::dashboard::DashboardRepository;
use crate::state::PortalState;

/// Default grace period for key rotation (24 hours).
const DEFAULT_GRACE_HOURS: i64 = 24;

/// Protected API key management routes (session auth required).
/// Supports both org-scoped paths (`/orgs/:org_id/keys`) and flat paths (`/api-keys`).
pub fn api_key_routes() -> Router<Arc<PortalState>> {
    Router::new()
        // Org-scoped routes
        .route("/orgs/:org_id/keys", post(create_key).get(list_keys))
        .route("/orgs/:org_id/keys/:id", delete(revoke_key))
        .route("/orgs/:org_id/keys/:id/rotate", post(rotate_key))
        // Flat routes (org_id from session claims)
        .route("/api-keys", post(create_key_flat).get(list_keys_flat))
        .route("/api-keys/:id", delete(revoke_key_flat))
        .route("/api-keys/:id/rotate", post(rotate_key_flat))
}

/// POST /portal/v1/orgs/:org_id/keys
///
/// Create a new API key. The raw key is returned ONCE in the response.
async fn create_key(
    State(state): State<Arc<PortalState>>,
    claims: axum::Extension<SessionClaims>,
    Path(org_id): Path<uuid::Uuid>,
    Json(req): Json<CreateKeyRequest>,
) -> Result<Json<CreateKeyResponse>, AppError> {
    verify_org_access(&claims, &org_id)?;
    require_api_key_management(&claims)?;

    if req.name.trim().is_empty() {
        return Err(AppError::BadRequest("name is required".to_string()));
    }

    if req.scopes.is_empty() {
        return Err(AppError::BadRequest(
            "At least one scope is required".to_string(),
        ));
    }

    if let Err(invalid) = validate_scopes(&req.scopes) {
        return Err(AppError::BadRequest(format!(
            "Invalid scopes: {}",
            invalid.join(", ")
        )));
    }

    let raw_key = generate_raw_key();
    let prefix = key_prefix(&raw_key);
    let hash = hash_key(&raw_key);

    let id = uuid::Uuid::new_v4();
    let key = state
        .api_key_repo
        .create(
            id,
            org_id,
            claims.tenant_id,
            req.name.clone(),
            prefix.clone(),
            hash,
            req.scopes.clone(),
        )
        .await
        .map_err(AppError::internal)?;

    // Best-effort audit log
    audit_log(
        &state.dashboard_repo,
        org_id,
        &claims.sub,
        "api_key.created",
        "api_key",
        Some(key.id.to_string()),
        Some(serde_json::json!({"name": key.name, "scopes": key.scopes})),
    )
    .await;

    Ok(Json(CreateKeyResponse {
        id: key.id,
        name: key.name,
        key_prefix: prefix,
        raw_key,
        scopes: key.scopes,
        created_at: key.created_at,
    }))
}

/// GET /portal/v1/orgs/:org_id/keys
///
/// List all API keys for an organization. Returns key prefixes (hints) only.
async fn list_keys(
    State(state): State<Arc<PortalState>>,
    claims: axum::Extension<SessionClaims>,
    Path(org_id): Path<uuid::Uuid>,
) -> Result<Json<Vec<KeyListItem>>, AppError> {
    verify_org_access(&claims, &org_id)?;

    let keys = state
        .api_key_repo
        .list_by_org(org_id)
        .await
        .map_err(AppError::internal)?;

    let items: Vec<KeyListItem> = keys
        .into_iter()
        .map(|k| KeyListItem {
            id: k.id,
            name: k.name,
            key_prefix: k.key_prefix,
            scopes: k.scopes,
            status: k.status,
            grace_expires_at: k.grace_expires_at,
            created_at: k.created_at,
        })
        .collect();

    Ok(Json(items))
}

/// DELETE /portal/v1/orgs/:org_id/keys/:id
///
/// Revoke an API key immediately.
async fn revoke_key(
    State(state): State<Arc<PortalState>>,
    claims: axum::Extension<SessionClaims>,
    Path((org_id, key_id)): Path<(uuid::Uuid, uuid::Uuid)>,
) -> Result<Json<serde_json::Value>, AppError> {
    verify_org_access(&claims, &org_id)?;
    require_api_key_management(&claims)?;

    let key = state
        .api_key_repo
        .find_by_id(key_id, org_id)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::NotFound("API key not found".to_string()))?;

    if key.status == KeyStatus::Revoked {
        return Err(AppError::Conflict("Key is already revoked".to_string()));
    }

    state
        .api_key_repo
        .update_status(key_id, KeyStatus::Revoked, None)
        .await
        .map_err(AppError::internal)?;

    // Invalidate API key cache in Redis (best-effort)
    invalidate_api_key_cache(&state, &key.key_hash).await;

    // Best-effort audit log
    audit_log(
        &state.dashboard_repo,
        org_id,
        &claims.sub,
        "api_key.revoked",
        "api_key",
        Some(key_id.to_string()),
        Some(serde_json::json!({"name": key.name})),
    )
    .await;

    Ok(Json(serde_json::json!({"message": "Key revoked"})))
}

/// POST /portal/v1/orgs/:org_id/keys/:id/rotate
///
/// Rotate an API key. Creates a new key and marks the old key as 'rotated'
/// with a grace period (default 24h) during which both keys work.
async fn rotate_key(
    State(state): State<Arc<PortalState>>,
    claims: axum::Extension<SessionClaims>,
    Path((org_id, old_key_id)): Path<(uuid::Uuid, uuid::Uuid)>,
) -> Result<Json<RotateKeyResponse>, AppError> {
    verify_org_access(&claims, &org_id)?;
    require_api_key_management(&claims)?;

    let old_key = state
        .api_key_repo
        .find_by_id(old_key_id, org_id)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::NotFound("API key not found".to_string()))?;

    if old_key.status != KeyStatus::Active {
        return Err(AppError::BadRequest(
            "Only active keys can be rotated".to_string(),
        ));
    }

    let grace_expires_at = Utc::now() + Duration::hours(DEFAULT_GRACE_HOURS);

    // Mark old key as rotated with grace period
    state
        .api_key_repo
        .update_status(old_key_id, KeyStatus::Rotated, Some(grace_expires_at))
        .await
        .map_err(AppError::internal)?;

    // Invalidate old key's cache so the rotated status is fetched fresh
    invalidate_api_key_cache(&state, &old_key.key_hash).await;

    // Create new key with same scopes
    let raw_key = generate_raw_key();
    let prefix = key_prefix(&raw_key);
    let hash = hash_key(&raw_key);
    let new_id = uuid::Uuid::new_v4();

    let new_key = state
        .api_key_repo
        .create(
            new_id,
            org_id,
            claims.tenant_id,
            format!("{} (rotated)", old_key.name),
            prefix.clone(),
            hash,
            old_key.scopes.clone(),
        )
        .await
        .map_err(AppError::internal)?;

    // Best-effort audit log
    audit_log(
        &state.dashboard_repo,
        org_id,
        &claims.sub,
        "api_key.rotated",
        "api_key",
        Some(old_key.id.to_string()),
        Some(serde_json::json!({
            "old_key_name": old_key.name,
            "new_key_id": new_key.id.to_string(),
            "grace_expires_at": grace_expires_at.to_rfc3339()
        })),
    )
    .await;

    Ok(Json(RotateKeyResponse {
        new_key: CreateKeyResponse {
            id: new_key.id,
            name: new_key.name,
            key_prefix: prefix,
            raw_key,
            scopes: new_key.scopes,
            created_at: new_key.created_at,
        },
        old_key_id: old_key.id,
        grace_expires_at,
    }))
}

// ─── Flat route handlers (org_id from session claims) ───

fn parse_org_id(claims: &SessionClaims) -> Result<uuid::Uuid, AppError> {
    claims
        .org_id
        .parse()
        .map_err(|_| AppError::internal("Invalid org_id in session"))
}

/// POST /portal/v1/api-keys
async fn create_key_flat(
    state: State<Arc<PortalState>>,
    claims: axum::Extension<SessionClaims>,
    json: Json<CreateKeyRequest>,
) -> Result<Json<CreateKeyResponse>, AppError> {
    let org_id = parse_org_id(&claims)?;
    create_key(state, claims, Path(org_id), json).await
}

/// GET /portal/v1/api-keys
async fn list_keys_flat(
    state: State<Arc<PortalState>>,
    claims: axum::Extension<SessionClaims>,
) -> Result<Json<Vec<KeyListItem>>, AppError> {
    let org_id = parse_org_id(&claims)?;
    list_keys(state, claims, Path(org_id)).await
}

/// DELETE /portal/v1/api-keys/:id
async fn revoke_key_flat(
    state: State<Arc<PortalState>>,
    claims: axum::Extension<SessionClaims>,
    Path(key_id): Path<uuid::Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let org_id = parse_org_id(&claims)?;
    revoke_key(state, claims, Path((org_id, key_id))).await
}

/// POST /portal/v1/api-keys/:id/rotate
async fn rotate_key_flat(
    state: State<Arc<PortalState>>,
    claims: axum::Extension<SessionClaims>,
    Path(key_id): Path<uuid::Uuid>,
) -> Result<Json<RotateKeyResponse>, AppError> {
    let org_id = parse_org_id(&claims)?;
    rotate_key(state, claims, Path((org_id, key_id))).await
}

/// Invalidate the API key cache entry in Redis (best-effort).
/// The cache key format matches `api_key_resolver` middleware: `gw:api_key:{hash}`.
async fn invalidate_api_key_cache(state: &PortalState, key_hash: &str) {
    if let Some(ref client) = state.redis_client {
        let cache_key = format!("gw:api_key:{}", key_hash);
        if let Err(e) = crate::redis_ops::del_key(client, &cache_key).await {
            tracing::warn!("Failed to invalidate API key cache: {}", e);
        }
    }
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

/// Verify that the caller's session belongs to the given org.
fn verify_org_access(claims: &SessionClaims, org_id: &uuid::Uuid) -> Result<(), AppError> {
    if claims.org_id != org_id.to_string() {
        return Err(AppError::Forbidden(
            "Not authorized to access this organization's keys".to_string(),
        ));
    }
    Ok(())
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
    use crate::models::api_key::ApiKey;
    use crate::models::auth::SessionClaims;
    use crate::repo::api_key::MockApiKeyRepository;
    use crate::repo::dashboard::MockDashboardRepository;
    use crate::repo::member::MockMemberRepository;
    use crate::repo::org::MockOrgRepository;

    fn make_state(api_key_repo: MockApiKeyRepository) -> Arc<PortalState> {
        let mut mock_dashboard = MockDashboardRepository::new();
        mock_dashboard
            .expect_insert_audit_log()
            .returning(|_| Ok(()));
        Arc::new(PortalState {
            org_repo: Arc::new(MockOrgRepository::new()),
            member_repo: Arc::new(MockMemberRepository::new()),
            api_key_repo: Arc::new(api_key_repo),
            dashboard_repo: Arc::new(mock_dashboard),
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
            exp: (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp() as usize,
        }
    }

    fn test_api_key(id: uuid::Uuid, org_id: uuid::Uuid) -> ApiKey {
        ApiKey {
            id,
            org_id,
            tenant_id: 1,
            name: "test-key".to_string(),
            key_prefix: "bk_live_test1234".to_string(),
            key_hash: "hash".to_string(),
            scopes: vec!["accounts:read".to_string()],
            status: KeyStatus::Active,
            grace_expires_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn key_app(state: Arc<PortalState>) -> Router {
        Router::new()
            .merge(api_key_routes())
            .route_layer(axum_mw::from_fn_with_state(
                Arc::clone(&state),
                session_auth,
            ))
            .with_state(state)
    }

    // --- Create Key Tests ---

    #[tokio::test]
    async fn test_create_key_success() {
        let mut mock_repo = MockApiKeyRepository::new();
        mock_repo.expect_create().returning(
            |id, org_id, tenant_id, name, key_prefix, key_hash, scopes| {
                Ok(ApiKey {
                    id,
                    org_id,
                    tenant_id,
                    name,
                    key_prefix,
                    key_hash,
                    scopes,
                    status: KeyStatus::Active,
                    grace_expires_at: None,
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                })
            },
        );

        let state = make_state(mock_repo);
        let org_id = uuid::Uuid::new_v4();
        let csrf = "csrf123";
        let claims = owner_claims(&org_id.to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = key_app(state);

        let body = serde_json::json!({
            "name": "Production Key",
            "scopes": ["accounts:read", "ledgers:read"]
        });

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri(format!("/orgs/{org_id}/keys"))
                    .header("Cookie", format!("portal_session={jwt}"))
                    .header("X-CSRF-Token", csrf)
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(resp["raw_key"].as_str().unwrap().starts_with("bk_live_"));
    }

    #[tokio::test]
    async fn test_create_key_empty_name() {
        let state = make_state(MockApiKeyRepository::new());
        let org_id = uuid::Uuid::new_v4();
        let csrf = "csrf123";
        let claims = owner_claims(&org_id.to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = key_app(state);

        let body = serde_json::json!({"name": "", "scopes": ["accounts:read"]});
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri(format!("/orgs/{org_id}/keys"))
                    .header("Cookie", format!("portal_session={jwt}"))
                    .header("X-CSRF-Token", csrf)
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_create_key_empty_scopes() {
        let state = make_state(MockApiKeyRepository::new());
        let org_id = uuid::Uuid::new_v4();
        let csrf = "csrf123";
        let claims = owner_claims(&org_id.to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = key_app(state);

        let body = serde_json::json!({"name": "Key", "scopes": []});
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri(format!("/orgs/{org_id}/keys"))
                    .header("Cookie", format!("portal_session={jwt}"))
                    .header("X-CSRF-Token", csrf)
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_create_key_invalid_scopes() {
        let state = make_state(MockApiKeyRepository::new());
        let org_id = uuid::Uuid::new_v4();
        let csrf = "csrf123";
        let claims = owner_claims(&org_id.to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = key_app(state);

        let body = serde_json::json!({"name": "Key", "scopes": ["invalid:scope"]});
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri(format!("/orgs/{org_id}/keys"))
                    .header("Cookie", format!("portal_session={jwt}"))
                    .header("X-CSRF-Token", csrf)
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_create_key_wrong_org() {
        let state = make_state(MockApiKeyRepository::new());
        let different_org_id = uuid::Uuid::new_v4();
        let csrf = "csrf123";
        let claims = owner_claims(&uuid::Uuid::new_v4().to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = key_app(state);

        let body = serde_json::json!({"name": "Key", "scopes": ["accounts:read"]});
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri(format!("/orgs/{different_org_id}/keys"))
                    .header("Cookie", format!("portal_session={jwt}"))
                    .header("X-CSRF-Token", csrf)
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    // --- List Keys Tests ---

    #[tokio::test]
    async fn test_list_keys_success() {
        let org_id = uuid::Uuid::new_v4();
        let mut mock_repo = MockApiKeyRepository::new();
        let key = test_api_key(uuid::Uuid::new_v4(), org_id);
        mock_repo
            .expect_list_by_org()
            .returning(move |_| Ok(vec![key.clone()]));

        let state = make_state(mock_repo);
        let csrf = "csrf123";
        let claims = owner_claims(&org_id.to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = key_app(state);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri(format!("/orgs/{org_id}/keys"))
                    .header("Cookie", format!("portal_session={jwt}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    // --- Revoke Key Tests ---

    #[tokio::test]
    async fn test_revoke_key_success() {
        let org_id = uuid::Uuid::new_v4();
        let key_id = uuid::Uuid::new_v4();
        let mut mock_repo = MockApiKeyRepository::new();

        let key = test_api_key(key_id, org_id);
        mock_repo
            .expect_find_by_id()
            .returning(move |_, _| Ok(Some(key.clone())));
        mock_repo
            .expect_update_status()
            .returning(|_, _, _| Ok(None));

        let state = make_state(mock_repo);
        let csrf = "csrf123";
        let claims = owner_claims(&org_id.to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = key_app(state);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("DELETE")
                    .uri(format!("/orgs/{org_id}/keys/{key_id}"))
                    .header("Cookie", format!("portal_session={jwt}"))
                    .header("X-CSRF-Token", csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_revoke_key_not_found() {
        let org_id = uuid::Uuid::new_v4();
        let key_id = uuid::Uuid::new_v4();
        let mut mock_repo = MockApiKeyRepository::new();
        mock_repo.expect_find_by_id().returning(|_, _| Ok(None));

        let state = make_state(mock_repo);
        let csrf = "csrf123";
        let claims = owner_claims(&org_id.to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = key_app(state);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("DELETE")
                    .uri(format!("/orgs/{org_id}/keys/{key_id}"))
                    .header("Cookie", format!("portal_session={jwt}"))
                    .header("X-CSRF-Token", csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_revoke_already_revoked_key() {
        let org_id = uuid::Uuid::new_v4();
        let key_id = uuid::Uuid::new_v4();
        let mut mock_repo = MockApiKeyRepository::new();

        let mut key = test_api_key(key_id, org_id);
        key.status = KeyStatus::Revoked;
        mock_repo
            .expect_find_by_id()
            .returning(move |_, _| Ok(Some(key.clone())));

        let state = make_state(mock_repo);
        let csrf = "csrf123";
        let claims = owner_claims(&org_id.to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = key_app(state);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("DELETE")
                    .uri(format!("/orgs/{org_id}/keys/{key_id}"))
                    .header("Cookie", format!("portal_session={jwt}"))
                    .header("X-CSRF-Token", csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    // --- Rotate Key Tests ---

    #[tokio::test]
    async fn test_rotate_key_success() {
        let org_id = uuid::Uuid::new_v4();
        let key_id = uuid::Uuid::new_v4();
        let mut mock_repo = MockApiKeyRepository::new();

        let key = test_api_key(key_id, org_id);
        mock_repo
            .expect_find_by_id()
            .returning(move |_, _| Ok(Some(key.clone())));
        mock_repo
            .expect_update_status()
            .returning(|_, _, _| Ok(None));
        mock_repo.expect_create().returning(
            |id, org_id, tenant_id, name, key_prefix, key_hash, scopes| {
                Ok(ApiKey {
                    id,
                    org_id,
                    tenant_id,
                    name,
                    key_prefix,
                    key_hash,
                    scopes,
                    status: KeyStatus::Active,
                    grace_expires_at: None,
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                })
            },
        );

        let state = make_state(mock_repo);
        let csrf = "csrf123";
        let claims = owner_claims(&org_id.to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = key_app(state);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri(format!("/orgs/{org_id}/keys/{key_id}/rotate"))
                    .header("Cookie", format!("portal_session={jwt}"))
                    .header("X-CSRF-Token", csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(resp["new_key"]["raw_key"]
            .as_str()
            .unwrap()
            .starts_with("bk_live_"));
        assert!(resp["grace_expires_at"].is_string());
    }

    #[tokio::test]
    async fn test_rotate_revoked_key_fails() {
        let org_id = uuid::Uuid::new_v4();
        let key_id = uuid::Uuid::new_v4();
        let mut mock_repo = MockApiKeyRepository::new();

        let mut key = test_api_key(key_id, org_id);
        key.status = KeyStatus::Revoked;
        mock_repo
            .expect_find_by_id()
            .returning(move |_, _| Ok(Some(key.clone())));

        let state = make_state(mock_repo);
        let csrf = "csrf123";
        let claims = owner_claims(&org_id.to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = key_app(state);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri(format!("/orgs/{org_id}/keys/{key_id}/rotate"))
                    .header("Cookie", format!("portal_session={jwt}"))
                    .header("X-CSRF-Token", csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
