use std::sync::Arc;

use axum::{
    extract::{Path, State},
    routing::{delete, get, post},
    Json, Router,
};

use bankie_common::error::AppError;

use crate::middleware::rbac::require_member_management;
use crate::models::auth::SessionClaims;
use crate::models::dashboard::NewAuditLog;
use crate::models::member::{
    generate_invite_token, hash_invite_token, parse_assignable_role, InviteMemberRequest,
    InviteMemberResponse, MemberRole, MemberStatus, OrgMember, UpdateRoleRequest,
};
use crate::repo::dashboard::DashboardRepository;
use crate::state::PortalState;

/// Invite link validity period in days.
const INVITE_EXPIRY_DAYS: i64 = 7;

/// Protected member management routes (session auth required).
pub fn member_routes() -> Router<Arc<PortalState>> {
    Router::new()
        .route("/members", get(list_members))
        .route("/members/invite", post(invite_member))
        .route("/members/:id/role", post(change_role))
        .route("/members/:id", delete(remove_member))
        .route("/members/:id/resend-invite", post(resend_invite))
}

/// GET /portal/v1/members
///
/// List all members in the caller's organization. All roles can view.
async fn list_members(
    State(state): State<Arc<PortalState>>,
    claims: axum::Extension<SessionClaims>,
) -> Result<Json<Vec<OrgMember>>, AppError> {
    let org_id = parse_org_id(&claims)?;

    let members = state
        .member_repo
        .list_by_org(org_id)
        .await
        .map_err(AppError::internal)?;

    Ok(Json(members))
}

/// POST /portal/v1/members/invite
///
/// Invite a new member to the organization. Requires owner or admin role.
/// Creates a member with `pending` status, generates a one-time invite token,
/// and returns the invite link. The raw token is shown once; only the hash is stored.
async fn invite_member(
    State(state): State<Arc<PortalState>>,
    claims: axum::Extension<SessionClaims>,
    Json(req): Json<InviteMemberRequest>,
) -> Result<Json<InviteMemberResponse>, AppError> {
    require_member_management(&claims)?;
    let org_id = parse_org_id(&claims)?;

    // Validate email
    if req.email.trim().is_empty() || !req.email.contains('@') {
        return Err(AppError::BadRequest("Valid email is required".to_string()));
    }

    // Validate role (only admin or member can be assigned)
    let role = parse_assignable_role(&req.role)
        .ok_or_else(|| AppError::BadRequest("Role must be 'admin' or 'member'".to_string()))?;

    // Check if email already exists
    let existing = state
        .member_repo
        .find_by_email(req.email.clone())
        .await
        .map_err(AppError::internal)?;
    if existing.is_some() {
        return Err(AppError::Conflict("Email already registered".to_string()));
    }

    // Generate invite token
    let raw_token = generate_invite_token();
    let token_hash = hash_invite_token(&raw_token);
    let expires_at = chrono::Utc::now() + chrono::Duration::days(INVITE_EXPIRY_DAYS);

    // Create member with pending status and placeholder password hash.
    let member_id = uuid::Uuid::new_v4();
    let role_str = serde_json::to_value(&role)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| "member".to_string());

    let member = state
        .member_repo
        .create_with_status(
            member_id,
            org_id,
            req.email.clone(),
            "pending_invite".to_string(),
            role_str,
            "pending".to_string(),
            Some(token_hash),
            Some(expires_at),
        )
        .await
        .map_err(AppError::internal)?;

    // Best-effort audit log
    audit_log(
        &state.dashboard_repo,
        org_id,
        &claims.sub,
        "member.invited",
        "member",
        Some(member.id.to_string()),
        Some(serde_json::json!({"email": req.email, "role": req.role})),
    )
    .await;

    let invite_link = format!("/invite?token={raw_token}");

    Ok(Json(InviteMemberResponse {
        member,
        invite_link,
    }))
}

/// POST /portal/v1/members/:id/role
///
/// Change a member's role. Rules:
/// - Owner can change anyone's role (except their own owner status).
/// - Admin can only change members to/from member role (not admin/owner).
/// - Members cannot change roles.
/// - Owner role cannot be assigned to anyone.
async fn change_role(
    State(state): State<Arc<PortalState>>,
    claims: axum::Extension<SessionClaims>,
    Path(member_id): Path<uuid::Uuid>,
    Json(req): Json<UpdateRoleRequest>,
) -> Result<Json<OrgMember>, AppError> {
    let caller_role = require_member_management(&claims)?;
    let org_id = parse_org_id(&claims)?;

    // Validate the requested role
    let new_role = parse_assignable_role(&req.role)
        .ok_or_else(|| AppError::BadRequest("Role must be 'admin' or 'member'".to_string()))?;

    // Find the target member
    let target = state
        .member_repo
        .find_by_id_and_org(member_id, org_id)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::NotFound("Member not found".to_string()))?;

    // Cannot change the owner's role
    if target.role == MemberRole::Owner {
        return Err(AppError::Forbidden(
            "Cannot change the owner's role".to_string(),
        ));
    }

    // Admin cannot promote to admin (only owner can)
    if caller_role == MemberRole::Admin && new_role == MemberRole::Admin {
        return Err(AppError::Forbidden(
            "Only the owner can promote members to admin".to_string(),
        ));
    }

    // Admin cannot demote other admins
    if caller_role == MemberRole::Admin && target.role == MemberRole::Admin {
        return Err(AppError::Forbidden(
            "Only the owner can change an admin's role".to_string(),
        ));
    }

    let role_str = serde_json::to_value(&new_role)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| "member".to_string());

    let updated = state
        .member_repo
        .update_role(member_id, role_str)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::NotFound("Member not found".to_string()))?;

    // Best-effort audit log
    audit_log(
        &state.dashboard_repo,
        org_id,
        &claims.sub,
        "member.role_changed",
        "member",
        Some(member_id.to_string()),
        Some(serde_json::json!({
            "old_role": target.role,
            "new_role": new_role,
            "email": target.email
        })),
    )
    .await;

    Ok(Json(updated))
}

/// DELETE /portal/v1/members/:id
///
/// Remove a member from the organization. Rules:
/// - Owner can remove anyone except themselves.
/// - Admin can remove members (not other admins or owner).
/// - Members cannot remove anyone.
/// - Cannot remove yourself.
async fn remove_member(
    State(state): State<Arc<PortalState>>,
    claims: axum::Extension<SessionClaims>,
    Path(member_id): Path<uuid::Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let caller_role = require_member_management(&claims)?;
    let org_id = parse_org_id(&claims)?;

    // Cannot remove yourself
    if claims.sub == member_id.to_string() {
        return Err(AppError::BadRequest(
            "Cannot remove yourself from the organization".to_string(),
        ));
    }

    // Find the target member
    let target = state
        .member_repo
        .find_by_id_and_org(member_id, org_id)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::NotFound("Member not found".to_string()))?;

    // Cannot remove the owner
    if target.role == MemberRole::Owner {
        return Err(AppError::Forbidden(
            "Cannot remove the organization owner".to_string(),
        ));
    }

    // Admin cannot remove other admins
    if caller_role == MemberRole::Admin && target.role == MemberRole::Admin {
        return Err(AppError::Forbidden(
            "Only the owner can remove admins".to_string(),
        ));
    }

    state
        .member_repo
        .delete(member_id)
        .await
        .map_err(AppError::internal)?;

    // Best-effort audit log
    audit_log(
        &state.dashboard_repo,
        org_id,
        &claims.sub,
        "member.removed",
        "member",
        Some(member_id.to_string()),
        Some(serde_json::json!({"email": target.email, "role": target.role})),
    )
    .await;

    Ok(Json(serde_json::json!({"message": "Member removed"})))
}

/// POST /portal/v1/members/:id/resend-invite
///
/// Resend an invitation to a pending member. Owner/admin only.
/// Regenerates the invite token and expiry, returns the new invite link.
async fn resend_invite(
    State(state): State<Arc<PortalState>>,
    claims: axum::Extension<SessionClaims>,
    Path(member_id): Path<uuid::Uuid>,
) -> Result<Json<InviteMemberResponse>, AppError> {
    require_member_management(&claims)?;
    let org_id = parse_org_id(&claims)?;

    let target = state
        .member_repo
        .find_by_id_and_org(member_id, org_id)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::NotFound("Member not found".to_string()))?;

    if target.status != MemberStatus::Pending {
        return Err(AppError::BadRequest(
            "Can only resend invites to pending members".to_string(),
        ));
    }

    // Generate new invite token
    let raw_token = generate_invite_token();
    let token_hash = hash_invite_token(&raw_token);
    let expires_at = chrono::Utc::now() + chrono::Duration::days(INVITE_EXPIRY_DAYS);

    let updated = state
        .member_repo
        .update_invite_token(member_id, token_hash, expires_at)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::NotFound("Member not found".to_string()))?;

    // Best-effort audit log
    audit_log(
        &state.dashboard_repo,
        org_id,
        &claims.sub,
        "member.invite_resent",
        "member",
        Some(member_id.to_string()),
        Some(serde_json::json!({"email": target.email})),
    )
    .await;

    let invite_link = format!("/invite?token={raw_token}");

    Ok(Json(InviteMemberResponse {
        member: updated,
        invite_link,
    }))
}

fn parse_org_id(claims: &SessionClaims) -> Result<uuid::Uuid, AppError> {
    claims
        .org_id
        .parse()
        .map_err(|_| AppError::internal("Invalid org_id in session"))
}

/// Best-effort audit log insertion.
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
    use crate::models::member::parse_role;
    use crate::repo::api_key::MockApiKeyRepository;
    use crate::repo::dashboard::MockDashboardRepository;
    use crate::repo::member::MockMemberRepository;
    use crate::repo::org::MockOrgRepository;

    fn make_state(member_repo: MockMemberRepository) -> Arc<PortalState> {
        let mut mock_dashboard = MockDashboardRepository::new();
        mock_dashboard
            .expect_insert_audit_log()
            .returning(|_| Ok(()));
        Arc::new(PortalState {
            org_repo: Arc::new(MockOrgRepository::new()),
            member_repo: Arc::new(member_repo),
            api_key_repo: Arc::new(MockApiKeyRepository::new()),
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

    fn test_claims(role: &str, org_id: &str, csrf: &str) -> SessionClaims {
        SessionClaims {
            sub: uuid::Uuid::new_v4().to_string(),
            org_id: org_id.to_string(),
            tenant_id: 1,
            role: role.to_string(),
            csrf: csrf.to_string(),
            exp: (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp() as usize,
        }
    }

    fn test_claims_with_sub(role: &str, org_id: &str, csrf: &str, sub: &str) -> SessionClaims {
        SessionClaims {
            sub: sub.to_string(),
            org_id: org_id.to_string(),
            tenant_id: 1,
            role: role.to_string(),
            csrf: csrf.to_string(),
            exp: (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp() as usize,
        }
    }

    fn test_member(id: uuid::Uuid, org_id: uuid::Uuid, role: MemberRole) -> OrgMember {
        OrgMember {
            id,
            org_id,
            email: "test@example.com".to_string(),
            password_hash: "hash".to_string(),
            role,
            status: MemberStatus::Active,
            invite_token_hash: None,
            invite_expires_at: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    fn pending_member(id: uuid::Uuid, org_id: uuid::Uuid) -> OrgMember {
        OrgMember {
            id,
            org_id,
            email: "pending@example.com".to_string(),
            password_hash: "pending_invite".to_string(),
            role: MemberRole::Member,
            status: MemberStatus::Pending,
            invite_token_hash: Some("somehash".to_string()),
            invite_expires_at: Some(chrono::Utc::now() + chrono::Duration::days(7)),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    fn member_app(state: Arc<PortalState>) -> axum::Router {
        axum::Router::new()
            .merge(member_routes())
            .route_layer(axum_mw::from_fn_with_state(
                Arc::clone(&state),
                session_auth,
            ))
            .with_state(state)
    }

    // --- List Members ---

    #[tokio::test]
    async fn test_list_members_all_roles_allowed() {
        let org_id = uuid::Uuid::new_v4();
        let mut mock = MockMemberRepository::new();
        let member = test_member(uuid::Uuid::new_v4(), org_id, MemberRole::Owner);
        mock.expect_list_by_org()
            .returning(move |_| Ok(vec![member.clone()]));

        let state = make_state(mock);
        let csrf = "csrf123";
        let claims = test_claims("member", &org_id.to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = member_app(state);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/members")
                    .header("Cookie", format!("portal_session={jwt}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    // --- Invite Member ---

    #[tokio::test]
    async fn test_invite_member_success() {
        let org_id = uuid::Uuid::new_v4();
        let mut mock = MockMemberRepository::new();
        mock.expect_find_by_email().returning(|_| Ok(None));
        mock.expect_create_with_status().returning(
            |id, org_id, email, password_hash, role, _status, _token_hash, _expires_at| {
                Ok(OrgMember {
                    id,
                    org_id,
                    email,
                    password_hash,
                    role: parse_role(&role),
                    status: MemberStatus::Pending,
                    invite_token_hash: _token_hash,
                    invite_expires_at: _expires_at,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                })
            },
        );

        let state = make_state(mock);
        let csrf = "csrf123";
        let claims = test_claims("owner", &org_id.to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = member_app(state);

        let body = serde_json::json!({"email": "new@example.com", "role": "member"});
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/members/invite")
                    .header("Cookie", format!("portal_session={jwt}"))
                    .header("X-CSRF-Token", csrf)
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Verify response contains invite_link
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let resp: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(resp["invite_link"]
            .as_str()
            .unwrap()
            .starts_with("/invite?token="));
        assert!(resp["member"]["email"].as_str().is_some());
    }

    #[tokio::test]
    async fn test_invite_member_denied_for_member_role() {
        let org_id = uuid::Uuid::new_v4();
        let state = make_state(MockMemberRepository::new());
        let csrf = "csrf123";
        let claims = test_claims("member", &org_id.to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = member_app(state);

        let body = serde_json::json!({"email": "new@example.com", "role": "member"});
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/members/invite")
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
    async fn test_invite_member_invalid_email() {
        let org_id = uuid::Uuid::new_v4();
        let state = make_state(MockMemberRepository::new());
        let csrf = "csrf123";
        let claims = test_claims("owner", &org_id.to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = member_app(state);

        let body = serde_json::json!({"email": "not-an-email", "role": "member"});
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/members/invite")
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
    async fn test_invite_member_invalid_role() {
        let org_id = uuid::Uuid::new_v4();
        let state = make_state(MockMemberRepository::new());
        let csrf = "csrf123";
        let claims = test_claims("owner", &org_id.to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = member_app(state);

        let body = serde_json::json!({"email": "new@example.com", "role": "owner"});
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/members/invite")
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
    async fn test_invite_member_duplicate_email() {
        let org_id = uuid::Uuid::new_v4();
        let mut mock = MockMemberRepository::new();
        let existing = test_member(uuid::Uuid::new_v4(), org_id, MemberRole::Member);
        mock.expect_find_by_email()
            .returning(move |_| Ok(Some(existing.clone())));

        let state = make_state(mock);
        let csrf = "csrf123";
        let claims = test_claims("owner", &org_id.to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = member_app(state);

        let body = serde_json::json!({"email": "test@example.com", "role": "member"});
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/members/invite")
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

    // --- Change Role ---

    #[tokio::test]
    async fn test_change_role_owner_can_promote_to_admin() {
        let org_id = uuid::Uuid::new_v4();
        let member_id = uuid::Uuid::new_v4();
        let mut mock = MockMemberRepository::new();

        let target = test_member(member_id, org_id, MemberRole::Member);
        mock.expect_find_by_id_and_org()
            .returning(move |_, _| Ok(Some(target.clone())));

        let updated = test_member(member_id, org_id, MemberRole::Admin);
        mock.expect_update_role()
            .returning(move |_, _| Ok(Some(updated.clone())));

        let state = make_state(mock);
        let csrf = "csrf123";
        let claims = test_claims("owner", &org_id.to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = member_app(state);

        let body = serde_json::json!({"role": "admin"});
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri(format!("/members/{member_id}/role"))
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
    async fn test_change_role_cannot_change_owner() {
        let org_id = uuid::Uuid::new_v4();
        let member_id = uuid::Uuid::new_v4();
        let mut mock = MockMemberRepository::new();

        let target = test_member(member_id, org_id, MemberRole::Owner);
        mock.expect_find_by_id_and_org()
            .returning(move |_, _| Ok(Some(target.clone())));

        let state = make_state(mock);
        let csrf = "csrf123";
        let claims = test_claims("owner", &org_id.to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = member_app(state);

        let body = serde_json::json!({"role": "member"});
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri(format!("/members/{member_id}/role"))
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
    async fn test_change_role_admin_cannot_promote_to_admin() {
        let org_id = uuid::Uuid::new_v4();
        let member_id = uuid::Uuid::new_v4();
        let mut mock = MockMemberRepository::new();

        let target = test_member(member_id, org_id, MemberRole::Member);
        mock.expect_find_by_id_and_org()
            .returning(move |_, _| Ok(Some(target.clone())));

        let state = make_state(mock);
        let csrf = "csrf123";
        let claims = test_claims("admin", &org_id.to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = member_app(state);

        let body = serde_json::json!({"role": "admin"});
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri(format!("/members/{member_id}/role"))
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
    async fn test_change_role_admin_cannot_demote_admin() {
        let org_id = uuid::Uuid::new_v4();
        let member_id = uuid::Uuid::new_v4();
        let mut mock = MockMemberRepository::new();

        let target = test_member(member_id, org_id, MemberRole::Admin);
        mock.expect_find_by_id_and_org()
            .returning(move |_, _| Ok(Some(target.clone())));

        let state = make_state(mock);
        let csrf = "csrf123";
        let claims = test_claims("admin", &org_id.to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = member_app(state);

        let body = serde_json::json!({"role": "member"});
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri(format!("/members/{member_id}/role"))
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
    async fn test_change_role_member_denied() {
        let org_id = uuid::Uuid::new_v4();
        let member_id = uuid::Uuid::new_v4();
        let state = make_state(MockMemberRepository::new());
        let csrf = "csrf123";
        let claims = test_claims("member", &org_id.to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = member_app(state);

        let body = serde_json::json!({"role": "admin"});
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri(format!("/members/{member_id}/role"))
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

    // --- Remove Member ---

    #[tokio::test]
    async fn test_remove_member_success() {
        let org_id = uuid::Uuid::new_v4();
        let member_id = uuid::Uuid::new_v4();
        let mut mock = MockMemberRepository::new();

        let target = test_member(member_id, org_id, MemberRole::Member);
        mock.expect_find_by_id_and_org()
            .returning(move |_, _| Ok(Some(target.clone())));
        mock.expect_delete().returning(|_| Ok(true));

        let state = make_state(mock);
        let csrf = "csrf123";
        let claims = test_claims("owner", &org_id.to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = member_app(state);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("DELETE")
                    .uri(format!("/members/{member_id}"))
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
    async fn test_remove_member_cannot_remove_self() {
        let org_id = uuid::Uuid::new_v4();
        let member_id = uuid::Uuid::new_v4();
        let state = make_state(MockMemberRepository::new());
        let csrf = "csrf123";
        let claims =
            test_claims_with_sub("owner", &org_id.to_string(), csrf, &member_id.to_string());
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = member_app(state);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("DELETE")
                    .uri(format!("/members/{member_id}"))
                    .header("Cookie", format!("portal_session={jwt}"))
                    .header("X-CSRF-Token", csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_remove_member_cannot_remove_owner() {
        let org_id = uuid::Uuid::new_v4();
        let member_id = uuid::Uuid::new_v4();
        let mut mock = MockMemberRepository::new();

        let target = test_member(member_id, org_id, MemberRole::Owner);
        mock.expect_find_by_id_and_org()
            .returning(move |_, _| Ok(Some(target.clone())));

        let state = make_state(mock);
        let csrf = "csrf123";
        let claims = test_claims("admin", &org_id.to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = member_app(state);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("DELETE")
                    .uri(format!("/members/{member_id}"))
                    .header("Cookie", format!("portal_session={jwt}"))
                    .header("X-CSRF-Token", csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_remove_member_admin_cannot_remove_admin() {
        let org_id = uuid::Uuid::new_v4();
        let member_id = uuid::Uuid::new_v4();
        let mut mock = MockMemberRepository::new();

        let target = test_member(member_id, org_id, MemberRole::Admin);
        mock.expect_find_by_id_and_org()
            .returning(move |_, _| Ok(Some(target.clone())));

        let state = make_state(mock);
        let csrf = "csrf123";
        let claims = test_claims("admin", &org_id.to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = member_app(state);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("DELETE")
                    .uri(format!("/members/{member_id}"))
                    .header("Cookie", format!("portal_session={jwt}"))
                    .header("X-CSRF-Token", csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_remove_member_denied_for_member_role() {
        let org_id = uuid::Uuid::new_v4();
        let member_id = uuid::Uuid::new_v4();
        let state = make_state(MockMemberRepository::new());
        let csrf = "csrf123";
        let claims = test_claims("member", &org_id.to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = member_app(state);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("DELETE")
                    .uri(format!("/members/{member_id}"))
                    .header("Cookie", format!("portal_session={jwt}"))
                    .header("X-CSRF-Token", csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    // --- Resend Invite ---

    #[tokio::test]
    async fn test_resend_invite_success() {
        let org_id = uuid::Uuid::new_v4();
        let member_id = uuid::Uuid::new_v4();
        let mut mock = MockMemberRepository::new();

        let target = pending_member(member_id, org_id);
        mock.expect_find_by_id_and_org()
            .returning(move |_, _| Ok(Some(target.clone())));

        let updated = pending_member(member_id, org_id);
        mock.expect_update_invite_token()
            .returning(move |_, _, _| Ok(Some(updated.clone())));

        let state = make_state(mock);
        let csrf = "csrf123";
        let claims = test_claims("owner", &org_id.to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = member_app(state);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri(format!("/members/{member_id}/resend-invite"))
                    .header("Cookie", format!("portal_session={jwt}"))
                    .header("X-CSRF-Token", csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Verify response contains new invite_link
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let resp: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(resp["invite_link"]
            .as_str()
            .unwrap()
            .starts_with("/invite?token="));
    }

    #[tokio::test]
    async fn test_resend_invite_not_pending() {
        let org_id = uuid::Uuid::new_v4();
        let member_id = uuid::Uuid::new_v4();
        let mut mock = MockMemberRepository::new();

        let target = test_member(member_id, org_id, MemberRole::Member);
        mock.expect_find_by_id_and_org()
            .returning(move |_, _| Ok(Some(target.clone())));

        let state = make_state(mock);
        let csrf = "csrf123";
        let claims = test_claims("owner", &org_id.to_string(), csrf);
        let jwt = make_jwt(&state.jwt_secret, &claims);
        let app = member_app(state);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri(format!("/members/{member_id}/resend-invite"))
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
