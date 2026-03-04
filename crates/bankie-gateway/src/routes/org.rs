use std::sync::Arc;

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};

use bankie_common::error::AppError;

use crate::middleware::rbac::require_org_management;
use crate::models::auth::SessionClaims;
use crate::models::dashboard::NewAuditLog;
use crate::models::org::{slugify, CreateOrgRequest, Organization, UpdateOrgRequest};
use crate::repo::dashboard::DashboardRepository;
use crate::state::PortalState;

/// Protected org management routes (session auth required).
pub fn org_routes() -> Router<Arc<PortalState>> {
    Router::new()
        .route("/orgs", post(create_org))
        .route("/orgs/:id", get(get_org).patch(update_org))
}

/// POST /portal/v1/orgs
///
/// Creates a new organization linked to a tenant. Requires session auth.
async fn create_org(
    State(state): State<Arc<PortalState>>,
    Json(req): Json<CreateOrgRequest>,
) -> Result<Json<Organization>, AppError> {
    if req.name.trim().is_empty() {
        return Err(AppError::BadRequest("name is required".to_string()));
    }

    let slug = slugify(&req.name);

    // Check for slug uniqueness
    let existing = state
        .org_repo
        .find_by_slug(slug.clone())
        .await
        .map_err(AppError::internal)?;
    if existing.is_some() {
        return Err(AppError::Conflict(format!(
            "Organization with slug '{slug}' already exists"
        )));
    }

    let id = uuid::Uuid::new_v4();
    let org = state
        .org_repo
        .create(id, 0, req.name, slug)
        .await
        .map_err(AppError::internal)?;

    Ok(Json(org))
}

/// GET /portal/v1/orgs/:id
///
/// Retrieve an organization by ID. Only accessible to members of the org.
async fn get_org(
    State(state): State<Arc<PortalState>>,
    claims: axum::Extension<SessionClaims>,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<Organization>, AppError> {
    let org = state
        .org_repo
        .find_by_id(id)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::NotFound("Organization not found".to_string()))?;

    // Verify the caller belongs to this org
    if claims.org_id != org.id.to_string() {
        return Err(AppError::Forbidden(
            "Not authorized to access this organization".to_string(),
        ));
    }

    Ok(Json(org))
}

/// PATCH /portal/v1/orgs/:id
///
/// Update organization name and/or status. Only accessible to org owner/admin.
async fn update_org(
    State(state): State<Arc<PortalState>>,
    claims: axum::Extension<SessionClaims>,
    Path(id): Path<uuid::Uuid>,
    Json(req): Json<UpdateOrgRequest>,
) -> Result<Json<Organization>, AppError> {
    // Verify org access
    let existing = state
        .org_repo
        .find_by_id(id)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::NotFound("Organization not found".to_string()))?;

    if claims.org_id != existing.id.to_string() {
        return Err(AppError::Forbidden(
            "Not authorized to modify this organization".to_string(),
        ));
    }

    // Only owner/admin can update
    require_org_management(&claims)?;

    if req.name.is_none() && req.status.is_none() {
        return Err(AppError::BadRequest(
            "At least one field must be provided".to_string(),
        ));
    }

    let org = state
        .org_repo
        .update(id, req.name.clone(), req.status.clone())
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::NotFound("Organization not found".to_string()))?;

    // Audit: org.updated
    let changes = serde_json::json!({
        "before": {"name": existing.name, "status": existing.status},
        "after": {"name": org.name, "status": org.status},
    });
    audit_log(
        &state.dashboard_repo,
        org.id,
        &claims.sub,
        "org.updated",
        "organization",
        Some(org.id.to_string()),
        Some(changes),
    )
    .await;

    Ok(Json(org))
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
    use crate::models::auth::SessionClaims;
    use crate::models::org::OrgStatus;
    use crate::repo::api_key::MockApiKeyRepository;
    use crate::repo::dashboard::MockDashboardRepository;
    use crate::repo::member::MockMemberRepository;
    use crate::repo::org::MockOrgRepository;
    use crate::repo::webhook::MockWebhookRepository;

    fn make_state(org_repo: MockOrgRepository) -> Arc<PortalState> {
        make_state_with_dashboard(org_repo, MockDashboardRepository::new())
    }

    fn make_state_with_dashboard(
        org_repo: MockOrgRepository,
        dashboard_repo: MockDashboardRepository,
    ) -> Arc<PortalState> {
        Arc::new(PortalState {
            org_repo: Arc::new(org_repo),
            member_repo: Arc::new(MockMemberRepository::new()),
            api_key_repo: Arc::new(MockApiKeyRepository::new()),
            dashboard_repo: Arc::new(dashboard_repo),
            webhook_repo: Arc::new(MockWebhookRepository::new()),
            jwt_secret: "test-secret-key-at-least-32-chars-long!!".to_string(),
            redis_client: None,
        })
    }

    fn mock_dashboard_with_audit() -> MockDashboardRepository {
        let mut mock = MockDashboardRepository::new();
        mock.expect_insert_audit_log().returning(|_| Ok(()));
        mock
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
            jti: String::new(),
        }
    }

    fn member_claims(org_id: &str, csrf: &str) -> SessionClaims {
        SessionClaims {
            sub: uuid::Uuid::new_v4().to_string(),
            name: "Test User".to_string(),
            org_id: org_id.to_string(),
            tenant_id: 1,
            role: "member".to_string(),
            csrf: csrf.to_string(),
            exp: (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp() as usize,
            jti: String::new(),
        }
    }

    fn test_org(id: uuid::Uuid, tenant_id: i32) -> Organization {
        Organization {
            id,
            tenant_id,
            name: "Test Org".to_string(),
            slug: "test-org".to_string(),
            status: OrgStatus::Active,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    fn org_app(state: Arc<PortalState>) -> Router {
        Router::new()
            .merge(org_routes())
            .route_layer(axum_mw::from_fn_with_state(
                Arc::clone(&state),
                session_auth,
            ))
            .with_state(state)
    }

    fn auth_headers(jwt: &str, csrf: &str) -> Vec<(&'static str, String)> {
        vec![
            ("Cookie", format!("portal_session={jwt}")),
            ("X-CSRF-Token", csrf.to_string()),
            ("Content-Type", "application/json".to_string()),
        ]
    }

    // --- Create Org Tests ---

    #[tokio::test]
    async fn test_create_org_success() {
        let mut org_repo = MockOrgRepository::new();
        org_repo.expect_find_by_slug().returning(|_| Ok(None));
        org_repo
            .expect_create()
            .returning(|id, tenant_id, name, slug| {
                Ok(Organization {
                    id,
                    tenant_id,
                    name,
                    slug,
                    status: OrgStatus::Active,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                })
            });

        let state = make_state(org_repo);
        let csrf = "csrf123";
        let claims = owner_claims(&uuid::Uuid::new_v4().to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = org_app(state);

        let body = serde_json::json!({"name": "New Org"});
        let headers = auth_headers(&jwt, csrf);

        let mut builder = HttpRequest::builder().method("POST").uri("/orgs");
        for (key, value) in &headers {
            builder = builder.header(*key, value.as_str());
        }
        let request = builder
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_create_org_empty_name() {
        let state = make_state(MockOrgRepository::new());
        let csrf = "csrf123";
        let claims = owner_claims(&uuid::Uuid::new_v4().to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = org_app(state);

        let body = serde_json::json!({"name": ""});
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/orgs")
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
    async fn test_create_org_duplicate_slug() {
        let mut org_repo = MockOrgRepository::new();
        let existing = test_org(uuid::Uuid::new_v4(), 1);
        org_repo
            .expect_find_by_slug()
            .returning(move |_| Ok(Some(existing.clone())));

        let state = make_state(org_repo);
        let csrf = "csrf123";
        let claims = owner_claims(&uuid::Uuid::new_v4().to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = org_app(state);

        let body = serde_json::json!({"name": "Test Org"});
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/orgs")
                    .header("Cookie", format!("portal_session={jwt}"))
                    .header("X-CSRF-Token", csrf)
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    // --- Get Org Tests ---

    #[tokio::test]
    async fn test_get_org_success() {
        let org_id = uuid::Uuid::new_v4();
        let mut org_repo = MockOrgRepository::new();
        let org = test_org(org_id, 1);
        org_repo
            .expect_find_by_id()
            .returning(move |_| Ok(Some(org.clone())));

        let state = make_state(org_repo);
        let claims = owner_claims(&org_id.to_string(), "csrf");
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = org_app(state);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri(format!("/orgs/{org_id}"))
                    .header("Cookie", format!("portal_session={jwt}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_org_not_found() {
        let mut org_repo = MockOrgRepository::new();
        org_repo.expect_find_by_id().returning(|_| Ok(None));

        let state = make_state(org_repo);
        let claims = owner_claims(&uuid::Uuid::new_v4().to_string(), "csrf");
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = org_app(state);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri(format!("/orgs/{}", uuid::Uuid::new_v4()))
                    .header("Cookie", format!("portal_session={jwt}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_get_org_wrong_org() {
        let org_id = uuid::Uuid::new_v4();
        let mut org_repo = MockOrgRepository::new();
        let org = test_org(org_id, 1);
        org_repo
            .expect_find_by_id()
            .returning(move |_| Ok(Some(org.clone())));

        let state = make_state(org_repo);
        // Claims have a different org_id
        let claims = owner_claims(&uuid::Uuid::new_v4().to_string(), "csrf");
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = org_app(state);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri(format!("/orgs/{org_id}"))
                    .header("Cookie", format!("portal_session={jwt}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    // --- Update Org Tests ---

    #[tokio::test]
    async fn test_update_org_success() {
        let org_id = uuid::Uuid::new_v4();
        let mut org_repo = MockOrgRepository::new();
        let org = test_org(org_id, 1);
        let org_clone = org.clone();

        org_repo
            .expect_find_by_id()
            .returning(move |_| Ok(Some(org.clone())));
        org_repo
            .expect_update()
            .returning(move |_id, name, status| {
                let mut updated = org_clone.clone();
                if let Some(n) = name {
                    updated.name = n;
                }
                if let Some(s) = status {
                    updated.status = s;
                }
                Ok(Some(updated))
            });

        let state = make_state_with_dashboard(org_repo, mock_dashboard_with_audit());
        let csrf = "csrf123";
        let claims = owner_claims(&org_id.to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = org_app(state);

        let body = serde_json::json!({"name": "Updated Name"});
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("PATCH")
                    .uri(format!("/orgs/{org_id}"))
                    .header("Cookie", format!("portal_session={jwt}"))
                    .header("X-CSRF-Token", csrf)
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_update_org_member_forbidden() {
        let org_id = uuid::Uuid::new_v4();
        let mut org_repo = MockOrgRepository::new();
        let org = test_org(org_id, 1);

        org_repo
            .expect_find_by_id()
            .returning(move |_| Ok(Some(org.clone())));

        let state = make_state(org_repo);
        let csrf = "csrf123";
        let claims = member_claims(&org_id.to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = org_app(state);

        let body = serde_json::json!({"name": "Updated"});
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("PATCH")
                    .uri(format!("/orgs/{org_id}"))
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

    #[tokio::test]
    async fn test_update_org_no_fields() {
        let org_id = uuid::Uuid::new_v4();
        let mut org_repo = MockOrgRepository::new();
        let org = test_org(org_id, 1);

        org_repo
            .expect_find_by_id()
            .returning(move |_| Ok(Some(org.clone())));

        let state = make_state(org_repo);
        let csrf = "csrf123";
        let claims = owner_claims(&org_id.to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = org_app(state);

        let body = serde_json::json!({});
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("PATCH")
                    .uri(format!("/orgs/{org_id}"))
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
}
