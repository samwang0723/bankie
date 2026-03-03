use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::header,
    response::Response,
    routing::{get, post},
    Json, Router,
};

use bankie_common::error::AppError;

use crate::models::auth::{
    AuthOrganization, AuthResponse, AuthUser, LoginRequest, SessionClaims, SignupRequest,
};
use crate::models::dashboard::NewAuditLog;
use crate::models::member::{
    hash_invite_token, AcceptInviteRequest, InviteInfo, MemberStatus, OrgMember,
};
use crate::models::org::{slugify, Organization};
use crate::repo::dashboard::DashboardRepository;
use crate::state::PortalState;

/// Public auth routes (no session required).
pub fn auth_routes() -> Router<Arc<PortalState>> {
    Router::new()
        .route("/auth/signup", post(signup))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/invite", get(validate_invite))
        .route("/auth/invite/accept", post(accept_invite))
}

/// POST /portal/v1/auth/signup
///
/// Creates a new organization and owner member. Returns the member info and
/// sets session + CSRF cookies.
async fn signup(
    State(state): State<Arc<PortalState>>,
    Json(req): Json<SignupRequest>,
) -> Result<Response, AppError> {
    // Validate input
    if req.org_name.trim().is_empty() {
        return Err(AppError::BadRequest("org_name is required".to_string()));
    }
    if req.email.trim().is_empty() || !req.email.contains('@') {
        return Err(AppError::BadRequest("Valid email is required".to_string()));
    }
    if req.password.len() < 8 {
        return Err(AppError::BadRequest(
            "Password must be at least 8 characters".to_string(),
        ));
    }

    // Check if email already exists
    let existing = state
        .member_repo
        .find_by_email(req.email.clone())
        .await
        .map_err(AppError::internal)?;
    if existing.is_some() {
        return Err(AppError::Conflict("Email already registered".to_string()));
    }

    // Create organization (tenant_id is auto-assigned by DB sequence)
    let org_id = uuid::Uuid::new_v4();
    let slug = slugify(&req.org_name);
    let org = state
        .org_repo
        .create(org_id, 0, req.org_name.clone(), slug)
        .await
        .map_err(AppError::internal)?;

    // Hash password
    let password_hash = hash_password(&req.password)?;

    // Create owner member
    let member_id = uuid::Uuid::new_v4();
    let member = state
        .member_repo
        .create(
            member_id,
            org.id,
            req.name.clone(),
            req.email.clone(),
            password_hash,
            "owner".to_string(),
        )
        .await
        .map_err(AppError::internal)?;

    // Audit: org.created
    audit_log(
        &state.dashboard_repo,
        org.id,
        &member.id.to_string(),
        "org.created",
        "organization",
        Some(org.id.to_string()),
        Some(serde_json::json!({"org_name": req.org_name, "email": req.email})),
    )
    .await;

    // Generate session
    build_session_response(&state, &member, &org)
}

/// Maximum failed login attempts per email before lockout.
const MAX_LOGIN_ATTEMPTS: i64 = 5;
/// Login lockout window in seconds (15 minutes).
const LOGIN_LOCKOUT_SECS: i64 = 900;

/// POST /portal/v1/auth/login
///
/// Verifies email/password, issues session JWT cookie and CSRF token.
/// Rate-limited: 5 failed attempts per email per 15-minute window.
async fn login(
    State(state): State<Arc<PortalState>>,
    Json(req): Json<LoginRequest>,
) -> Result<Response, AppError> {
    // Check login rate limit (per email)
    check_login_rate_limit(&state, &req.email).await?;

    let member = state
        .member_repo
        .find_by_email(req.email.clone())
        .await
        .map_err(AppError::internal)?;

    let member = match member {
        Some(m) => m,
        None => {
            record_failed_login(&state, &req.email).await;
            // Audit: login failed (unknown email)
            audit_log(
                &state.dashboard_repo,
                uuid::Uuid::nil(),
                &uuid::Uuid::nil().to_string(),
                "auth.login_failed",
                "member",
                None,
                Some(serde_json::json!({"email": req.email, "reason": "email_not_found"})),
            )
            .await;
            return Err(AppError::Unauthorized(
                "Invalid email or password".to_string(),
            ));
        }
    };

    if member.status != MemberStatus::Active {
        return Err(AppError::Forbidden("Account is suspended".to_string()));
    }

    if verify_password(&req.password, &member.password_hash).is_err() {
        record_failed_login(&state, &req.email).await;
        // Audit: login failed (wrong password)
        audit_log(
            &state.dashboard_repo,
            member.org_id,
            &member.id.to_string(),
            "auth.login_failed",
            "member",
            Some(member.id.to_string()),
            Some(serde_json::json!({"email": req.email, "reason": "invalid_password"})),
        )
        .await;
        return Err(AppError::Unauthorized(
            "Invalid email or password".to_string(),
        ));
    }

    // Resolve tenant_id from the organization
    let org = state
        .org_repo
        .find_by_id(member.org_id)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::internal("Organization not found for member"))?;

    // Audit: login success
    audit_log(
        &state.dashboard_repo,
        org.id,
        &member.id.to_string(),
        "auth.login_success",
        "member",
        Some(member.id.to_string()),
        Some(serde_json::json!({"email": member.email})),
    )
    .await;

    build_session_response(&state, &member, &org)
}

/// POST /portal/v1/auth/logout
///
/// Clears session and CSRF cookies. Best-effort audit log if session is valid.
async fn logout(
    State(state): State<Arc<PortalState>>,
    req_headers: axum::http::HeaderMap,
) -> Response {
    // Best-effort audit: try to extract session info from cookie
    if let Some(cookie_header) = req_headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(token) = cookie_header.split(';').find_map(|pair| {
            let pair = pair.trim();
            pair.strip_prefix("portal_session=")
        }) {
            if let Ok(token_data) = jsonwebtoken::decode::<SessionClaims>(
                token,
                &jsonwebtoken::DecodingKey::from_secret(state.jwt_secret.as_bytes()),
                &jsonwebtoken::Validation::default(),
            ) {
                let claims = token_data.claims;
                if let Ok(org_id) = claims.org_id.parse::<uuid::Uuid>() {
                    audit_log(
                        &state.dashboard_repo,
                        org_id,
                        &claims.sub,
                        "auth.logout",
                        "member",
                        Some(claims.sub.clone()),
                        None,
                    )
                    .await;
                }
            }
        }
    }

    let secure_flag = if is_secure_env() { "; Secure" } else { "" };
    let clear_session =
        format!("portal_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure_flag}");
    let clear_csrf = format!("csrf_token=; Path=/; SameSite=Lax; Max-Age=0{secure_flag}");

    let mut response =
        axum::response::Json(serde_json::json!({"message": "Logged out"})).into_response();
    let headers = response.headers_mut();
    headers.append(header::SET_COOKIE, clear_session.parse().unwrap());
    headers.append(header::SET_COOKIE, clear_csrf.parse().unwrap());
    response
}

/// Build a response with session JWT and CSRF cookies set.
fn build_session_response(
    state: &PortalState,
    member: &OrgMember,
    org: &Organization,
) -> Result<Response, AppError> {
    use jsonwebtoken::{encode, EncodingKey, Header};
    use rand::Rng;

    // Generate CSRF token
    let csrf_token: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();

    let exp = (chrono::Utc::now() + chrono::Duration::hours(24)).timestamp() as usize;

    let claims = SessionClaims {
        sub: member.id.to_string(),
        name: member.name.clone(),
        org_id: org.id.to_string(),
        tenant_id: org.tenant_id,
        role: serde_json::to_value(&member.role)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| "member".to_string()),
        csrf: csrf_token.clone(),
        exp,
    };

    let jwt = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(state.jwt_secret.as_bytes()),
    )
    .map_err(AppError::internal)?;

    let auth_resp = AuthResponse {
        user: AuthUser {
            id: member.id,
            name: member.name.clone(),
            email: member.email.clone(),
            role: member.role.clone(),
            created_at: member.created_at,
        },
        organization: AuthOrganization {
            id: org.id,
            name: org.name.clone(),
            slug: org.slug.clone(),
            environment: "live".to_string(),
            created_at: org.created_at,
        },
    };

    let secure_flag = if is_secure_env() { "; Secure" } else { "" };
    let session_cookie =
        format!("portal_session={jwt}; Path=/; HttpOnly; SameSite=Lax; Max-Age=86400{secure_flag}");
    let csrf_cookie =
        format!("csrf_token={csrf_token}; Path=/; SameSite=Lax; Max-Age=86400{secure_flag}");

    let mut response = axum::response::Json(auth_resp).into_response();
    let headers = response.headers_mut();
    headers.append(header::SET_COOKIE, session_cookie.parse().unwrap());
    headers.append(header::SET_COOKIE, csrf_cookie.parse().unwrap());

    Ok(response)
}

/// Check whether the email has exceeded the login attempt limit.
/// If Redis is unavailable, the check is skipped (fail-open for availability).
async fn check_login_rate_limit(state: &PortalState, email: &str) -> Result<(), AppError> {
    if let Some(ref client) = state.redis_client {
        let key = format!("login_attempts:{}", email);
        match crate::redis_ops::get_value(client, &key).await {
            Ok(Some(count_str)) => {
                if let Ok(count) = count_str.parse::<i64>() {
                    if count >= MAX_LOGIN_ATTEMPTS {
                        // Get remaining lockout seconds from Redis TTL
                        let retry_after = match crate::redis_ops::get_ttl(client, &key).await {
                            Ok(ttl) if ttl > 0 => ttl as u64,
                            _ => LOGIN_LOCKOUT_SECS as u64,
                        };
                        return Err(AppError::TooManyRequests(
                            "Too many login attempts. Please try again later.".to_string(),
                            retry_after,
                        ));
                    }
                }
            }
            Ok(None) => {} // No attempts recorded
            Err(e) => {
                tracing::warn!("Redis error checking login rate limit: {}", e);
            }
        }
    }
    Ok(())
}

/// Record a failed login attempt for the given email.
async fn record_failed_login(state: &PortalState, email: &str) {
    if let Some(ref client) = state.redis_client {
        let key = format!("login_attempts:{}", email);
        if let Err(e) = crate::redis_ops::incr_with_expiry(client, &key, LOGIN_LOCKOUT_SECS).await {
            tracing::warn!("Redis error recording failed login: {}", e);
        }
    }
}

/// Query parameter for invite validation.
#[derive(Debug, serde::Deserialize)]
struct InviteQuery {
    token: String,
}

/// GET /portal/v1/auth/invite?token=<raw>
///
/// Public endpoint. Hash the token, look up the member, validate not expired.
/// Returns `{ email, org_name, role }` for the accept form.
async fn validate_invite(
    State(state): State<Arc<PortalState>>,
    Query(query): Query<InviteQuery>,
) -> Result<Json<InviteInfo>, AppError> {
    let token_hash = hash_invite_token(&query.token);

    let member = state
        .member_repo
        .find_by_invite_token_hash(token_hash)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::NotFound("Invalid or expired invite link".to_string()))?;

    // Check expiry
    if let Some(expires_at) = member.invite_expires_at {
        if expires_at < chrono::Utc::now() {
            return Err(AppError::BadRequest(
                "Invite link has expired. Please ask the admin to resend the invite.".to_string(),
            ));
        }
    } else {
        return Err(AppError::NotFound(
            "Invalid or expired invite link".to_string(),
        ));
    }

    // Resolve org name
    let org = state
        .org_repo
        .find_by_id(member.org_id)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::internal("Organization not found for invite"))?;

    Ok(Json(InviteInfo {
        email: member.email,
        org_name: org.name,
        role: member.role,
    }))
}

/// POST /portal/v1/auth/invite/accept
///
/// Public endpoint. Accepts an invite: validates token, hashes password,
/// activates the member, and sets session cookies (same as login).
async fn accept_invite(
    State(state): State<Arc<PortalState>>,
    Json(req): Json<AcceptInviteRequest>,
) -> Result<Response, AppError> {
    if req.password.len() < 8 {
        return Err(AppError::BadRequest(
            "Password must be at least 8 characters".to_string(),
        ));
    }

    let token_hash = hash_invite_token(&req.token);

    let member = state
        .member_repo
        .find_by_invite_token_hash(token_hash)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::NotFound("Invalid or expired invite link".to_string()))?;

    // Check expiry
    if let Some(expires_at) = member.invite_expires_at {
        if expires_at < chrono::Utc::now() {
            return Err(AppError::BadRequest(
                "Invite link has expired. Please ask the admin to resend the invite.".to_string(),
            ));
        }
    } else {
        return Err(AppError::NotFound(
            "Invalid or expired invite link".to_string(),
        ));
    }

    // Hash password
    let password_hash = hash_password(&req.password)?;

    // Accept invite: sets status=active, clears token fields
    let activated = state
        .member_repo
        .accept_invite(member.id, req.name.clone(), password_hash)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::internal("Failed to activate member"))?;

    // Resolve organization for session
    let org = state
        .org_repo
        .find_by_id(activated.org_id)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::internal("Organization not found for member"))?;

    build_session_response(&state, &activated, &org)
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

/// Returns true when the deployment environment should use HTTPS (i.e. not local dev).
fn is_secure_env() -> bool {
    let env = std::env::var("ENV").unwrap_or_else(|_| "local".to_string());
    env != "local"
}

fn hash_password(password: &str) -> Result<String, AppError> {
    use argon2::{password_hash::SaltString, Argon2, PasswordHasher};
    use rand::rngs::OsRng;

    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    argon2
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(AppError::internal)
}

fn verify_password(password: &str, hash: &str) -> Result<(), AppError> {
    use argon2::{Argon2, PasswordHash, PasswordVerifier};

    let parsed = PasswordHash::new(hash).map_err(AppError::internal)?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .map_err(|_| AppError::Unauthorized("Invalid email or password".to_string()))
}

use axum::response::IntoResponse;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request as HttpRequest, StatusCode},
    };
    use tower::ServiceExt;

    use crate::models::member::MemberRole;
    use crate::models::org::OrgStatus;
    use crate::repo::api_key::MockApiKeyRepository;
    use crate::repo::dashboard::MockDashboardRepository;
    use crate::repo::member::MockMemberRepository;
    use crate::repo::org::MockOrgRepository;
    use crate::repo::webhook::MockWebhookRepository;

    fn test_state_with(
        org_repo: MockOrgRepository,
        member_repo: MockMemberRepository,
    ) -> Arc<PortalState> {
        test_state_with_dashboard(org_repo, member_repo, MockDashboardRepository::new())
    }

    fn test_state_with_dashboard(
        org_repo: MockOrgRepository,
        member_repo: MockMemberRepository,
        dashboard_repo: MockDashboardRepository,
    ) -> Arc<PortalState> {
        Arc::new(PortalState {
            org_repo: Arc::new(org_repo),
            member_repo: Arc::new(member_repo),
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

    fn test_member(id: uuid::Uuid, org_id: uuid::Uuid) -> OrgMember {
        OrgMember {
            id,
            org_id,
            name: "Test User".to_string(),
            email: "test@example.com".to_string(),
            password_hash: hash_password("password123").unwrap(),
            role: MemberRole::Owner,
            status: MemberStatus::Active,
            invite_token_hash: None,
            invite_expires_at: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    fn signup_app(state: Arc<PortalState>) -> Router {
        Router::new().merge(auth_routes()).with_state(state)
    }

    // --- Signup Tests ---

    #[tokio::test]
    async fn test_signup_success() {
        let mut org_repo = MockOrgRepository::new();
        let mut member_repo = MockMemberRepository::new();

        let org_id = uuid::Uuid::new_v4();
        member_repo.expect_find_by_email().returning(|_| Ok(None));

        let org_clone = test_org(org_id, 1);
        org_repo
            .expect_create()
            .returning(move |id, tenant_id, name, slug| {
                let mut org = org_clone.clone();
                org.id = id;
                org.tenant_id = tenant_id;
                org.name = name;
                org.slug = slug;
                Ok(org)
            });

        member_repo.expect_create().returning(
            move |id, org_id_arg, name, email, password_hash, _role| {
                Ok(OrgMember {
                    id,
                    org_id: org_id_arg,
                    name,
                    email,
                    password_hash,
                    role: MemberRole::Owner,
                    status: MemberStatus::Active,
                    invite_token_hash: None,
                    invite_expires_at: None,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                })
            },
        );

        let state = test_state_with_dashboard(org_repo, member_repo, mock_dashboard_with_audit());
        let app = signup_app(state);

        let body = serde_json::json!({
            "org_name": "Test Org",
            "name": "Test User",
            "email": "new@example.com",
            "password": "password123"
        });

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/auth/signup")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Verify session cookie is set
        let cookies: Vec<_> = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().unwrap().to_string())
            .collect();
        assert!(cookies.iter().any(|c| c.starts_with("portal_session=")));
        assert!(cookies.iter().any(|c| c.starts_with("csrf_token=")));
    }

    #[tokio::test]
    async fn test_signup_empty_org_name() {
        let state = test_state_with(MockOrgRepository::new(), MockMemberRepository::new());
        let app = signup_app(state);

        let body = serde_json::json!({
            "org_name": "",
            "name": "Test User",
            "email": "test@example.com",
            "password": "password123"
        });

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/auth/signup")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_signup_invalid_email() {
        let state = test_state_with(MockOrgRepository::new(), MockMemberRepository::new());
        let app = signup_app(state);

        let body = serde_json::json!({
            "org_name": "Test",
            "name": "Test User",
            "email": "not-an-email",
            "password": "password123"
        });

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/auth/signup")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_signup_short_password() {
        let state = test_state_with(MockOrgRepository::new(), MockMemberRepository::new());
        let app = signup_app(state);

        let body = serde_json::json!({
            "org_name": "Test",
            "name": "Test User",
            "email": "test@example.com",
            "password": "short"
        });

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/auth/signup")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_signup_duplicate_email() {
        let mut member_repo = MockMemberRepository::new();
        let existing_member = test_member(uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
        member_repo
            .expect_find_by_email()
            .returning(move |_| Ok(Some(existing_member.clone())));

        let state = test_state_with(MockOrgRepository::new(), member_repo);
        let app = signup_app(state);

        let body = serde_json::json!({
            "org_name": "Test",
            "name": "Test User",
            "email": "test@example.com",
            "password": "password123"
        });

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/auth/signup")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    // --- Login Tests ---

    #[tokio::test]
    async fn test_login_success() {
        let org_id = uuid::Uuid::new_v4();
        let mut org_repo = MockOrgRepository::new();
        let org = test_org(org_id, 42);
        org_repo
            .expect_find_by_id()
            .returning(move |_| Ok(Some(org.clone())));

        let mut member_repo = MockMemberRepository::new();
        let member = test_member(uuid::Uuid::new_v4(), org_id);
        member_repo
            .expect_find_by_email()
            .returning(move |_| Ok(Some(member.clone())));

        let state = test_state_with_dashboard(org_repo, member_repo, mock_dashboard_with_audit());
        let app = signup_app(state);

        let body = serde_json::json!({
            "email": "test@example.com",
            "password": "password123"
        });

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/auth/login")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let cookies: Vec<_> = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().unwrap().to_string())
            .collect();
        assert!(cookies.iter().any(|c| c.starts_with("portal_session=")));
        assert!(cookies.iter().any(|c| c.starts_with("csrf_token=")));
    }

    #[tokio::test]
    async fn test_login_wrong_password() {
        let mut member_repo = MockMemberRepository::new();
        let member = test_member(uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
        member_repo
            .expect_find_by_email()
            .returning(move |_| Ok(Some(member.clone())));

        let state = test_state_with_dashboard(
            MockOrgRepository::new(),
            member_repo,
            mock_dashboard_with_audit(),
        );
        let app = signup_app(state);

        let body = serde_json::json!({
            "email": "test@example.com",
            "password": "wrong_password"
        });

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/auth/login")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_login_email_not_found() {
        let mut member_repo = MockMemberRepository::new();
        member_repo.expect_find_by_email().returning(|_| Ok(None));

        let state = test_state_with_dashboard(
            MockOrgRepository::new(),
            member_repo,
            mock_dashboard_with_audit(),
        );
        let app = signup_app(state);

        let body = serde_json::json!({
            "email": "nonexistent@example.com",
            "password": "password123"
        });

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/auth/login")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_login_suspended_member() {
        let mut member_repo = MockMemberRepository::new();
        let mut member = test_member(uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
        member.status = MemberStatus::Suspended;
        member_repo
            .expect_find_by_email()
            .returning(move |_| Ok(Some(member.clone())));

        let state = test_state_with(MockOrgRepository::new(), member_repo);
        let app = signup_app(state);

        let body = serde_json::json!({
            "email": "test@example.com",
            "password": "password123"
        });

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/auth/login")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    // --- Logout Test ---

    #[tokio::test]
    async fn test_logout_clears_cookies() {
        let state = test_state_with(MockOrgRepository::new(), MockMemberRepository::new());
        let app = signup_app(state);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/auth/logout")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let cookies: Vec<_> = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().unwrap().to_string())
            .collect();
        assert!(cookies.iter().any(|c| c.contains("Max-Age=0")));
    }

    #[tokio::test]
    async fn test_logout_with_valid_session_logs_audit() {
        use jsonwebtoken::{encode, EncodingKey, Header};

        let jwt_secret = "test-secret-key-at-least-32-chars-long!!";
        let claims = SessionClaims {
            sub: uuid::Uuid::new_v4().to_string(),
            name: "Test User".to_string(),
            org_id: uuid::Uuid::new_v4().to_string(),
            tenant_id: 1,
            role: "owner".to_string(),
            csrf: "csrf123".to_string(),
            exp: (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp() as usize,
        };
        let jwt = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(jwt_secret.as_bytes()),
        )
        .unwrap();

        let state = test_state_with_dashboard(
            MockOrgRepository::new(),
            MockMemberRepository::new(),
            mock_dashboard_with_audit(),
        );
        let app = signup_app(state);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/auth/logout")
                    .header("Cookie", format!("portal_session={jwt}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let cookies: Vec<_> = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().unwrap().to_string())
            .collect();
        assert!(cookies.iter().any(|c| c.contains("Max-Age=0")));
    }

    // --- Password hashing ---

    #[test]
    fn test_hash_and_verify_password() {
        let hash = hash_password("test_password").unwrap();
        assert!(verify_password("test_password", &hash).is_ok());
    }

    #[test]
    fn test_verify_wrong_password_fails() {
        let hash = hash_password("correct_password").unwrap();
        assert!(verify_password("wrong_password", &hash).is_err());
    }

    // --- Validate Invite Tests ---

    #[tokio::test]
    async fn test_validate_invite_success() {
        use crate::models::member::hash_invite_token;

        let org_id = uuid::Uuid::new_v4();
        let raw_token = "abc123def456";
        let token_hash = hash_invite_token(raw_token);

        let mut member_repo = MockMemberRepository::new();
        let member = OrgMember {
            id: uuid::Uuid::new_v4(),
            org_id,
            name: String::new(),
            email: "invited@example.com".to_string(),
            password_hash: "pending_invite".to_string(),
            role: MemberRole::Member,
            status: MemberStatus::Pending,
            invite_token_hash: Some(token_hash),
            invite_expires_at: Some(chrono::Utc::now() + chrono::Duration::days(7)),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        member_repo
            .expect_find_by_invite_token_hash()
            .returning(move |_| Ok(Some(member.clone())));

        let mut org_repo = MockOrgRepository::new();
        let org = test_org(org_id, 1);
        org_repo
            .expect_find_by_id()
            .returning(move |_| Ok(Some(org.clone())));

        let state = test_state_with(org_repo, member_repo);
        let app = signup_app(state);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri(format!("/auth/invite?token={raw_token}"))
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
        assert_eq!(resp["email"], "invited@example.com");
        assert_eq!(resp["org_name"], "Test Org");
    }

    #[tokio::test]
    async fn test_validate_invite_expired() {
        use crate::models::member::hash_invite_token;

        let org_id = uuid::Uuid::new_v4();
        let raw_token = "expired_token_123";
        let token_hash = hash_invite_token(raw_token);

        let mut member_repo = MockMemberRepository::new();
        let member = OrgMember {
            id: uuid::Uuid::new_v4(),
            org_id,
            name: String::new(),
            email: "invited@example.com".to_string(),
            password_hash: "pending_invite".to_string(),
            role: MemberRole::Member,
            status: MemberStatus::Pending,
            invite_token_hash: Some(token_hash),
            invite_expires_at: Some(chrono::Utc::now() - chrono::Duration::hours(1)),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        member_repo
            .expect_find_by_invite_token_hash()
            .returning(move |_| Ok(Some(member.clone())));

        let state = test_state_with(MockOrgRepository::new(), member_repo);
        let app = signup_app(state);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri(format!("/auth/invite?token={raw_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_validate_invite_not_found() {
        let mut member_repo = MockMemberRepository::new();
        member_repo
            .expect_find_by_invite_token_hash()
            .returning(|_| Ok(None));

        let state = test_state_with(MockOrgRepository::new(), member_repo);
        let app = signup_app(state);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/auth/invite?token=nonexistent_token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    // --- Accept Invite Tests ---

    #[tokio::test]
    async fn test_accept_invite_success() {
        use crate::models::member::hash_invite_token;

        let org_id = uuid::Uuid::new_v4();
        let member_id = uuid::Uuid::new_v4();
        let raw_token = "valid_token_for_accept";
        let token_hash = hash_invite_token(raw_token);

        let mut member_repo = MockMemberRepository::new();
        let pending = OrgMember {
            id: member_id,
            org_id,
            name: String::new(),
            email: "invited@example.com".to_string(),
            password_hash: "pending_invite".to_string(),
            role: MemberRole::Member,
            status: MemberStatus::Pending,
            invite_token_hash: Some(token_hash),
            invite_expires_at: Some(chrono::Utc::now() + chrono::Duration::days(7)),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        member_repo
            .expect_find_by_invite_token_hash()
            .returning(move |_| Ok(Some(pending.clone())));

        let activated = OrgMember {
            id: member_id,
            org_id,
            name: "Invited User".to_string(),
            email: "invited@example.com".to_string(),
            password_hash: "argon2_hash".to_string(),
            role: MemberRole::Member,
            status: MemberStatus::Active,
            invite_token_hash: None,
            invite_expires_at: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        member_repo
            .expect_accept_invite()
            .returning(move |_, _, _| Ok(Some(activated.clone())));

        let mut org_repo = MockOrgRepository::new();
        let org = test_org(org_id, 1);
        org_repo
            .expect_find_by_id()
            .returning(move |_| Ok(Some(org.clone())));

        let state = test_state_with(org_repo, member_repo);
        let app = signup_app(state);

        let body = serde_json::json!({
            "token": raw_token,
            "password": "strongpassword123"
        });

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/auth/invite/accept")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Verify session cookies are set
        let cookies: Vec<_> = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().unwrap().to_string())
            .collect();
        assert!(cookies.iter().any(|c| c.starts_with("portal_session=")));
        assert!(cookies.iter().any(|c| c.starts_with("csrf_token=")));
    }

    #[tokio::test]
    async fn test_accept_invite_short_password() {
        let state = test_state_with(MockOrgRepository::new(), MockMemberRepository::new());
        let app = signup_app(state);

        let body = serde_json::json!({
            "token": "some_token",
            "password": "short"
        });

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/auth/invite/accept")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_accept_invite_invalid_token() {
        let mut member_repo = MockMemberRepository::new();
        member_repo
            .expect_find_by_invite_token_hash()
            .returning(|_| Ok(None));

        let state = test_state_with(MockOrgRepository::new(), member_repo);
        let app = signup_app(state);

        let body = serde_json::json!({
            "token": "nonexistent_token",
            "password": "strongpassword123"
        });

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/auth/invite/accept")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
