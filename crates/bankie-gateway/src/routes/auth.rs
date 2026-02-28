use std::sync::Arc;

use axum::{extract::State, http::header, response::Response, routing::post, Json, Router};

use bankie_common::error::AppError;

use crate::models::auth::{
    AuthOrganization, AuthResponse, AuthUser, LoginRequest, SessionClaims, SignupRequest,
};
use crate::models::member::{MemberStatus, OrgMember};
use crate::models::org::{slugify, Organization};
use crate::state::PortalState;

/// Public auth routes (no session required).
pub fn auth_routes() -> Router<Arc<PortalState>> {
    Router::new()
        .route("/auth/signup", post(signup))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
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
            req.email.clone(),
            password_hash,
            "owner".to_string(),
        )
        .await
        .map_err(AppError::internal)?;

    // Generate session
    build_session_response(&state, &member, &org)
}

/// POST /portal/v1/auth/login
///
/// Verifies email/password, issues session JWT cookie and CSRF token.
async fn login(
    State(state): State<Arc<PortalState>>,
    Json(req): Json<LoginRequest>,
) -> Result<Response, AppError> {
    let member = state
        .member_repo
        .find_by_email(req.email.clone())
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::Unauthorized("Invalid email or password".to_string()))?;

    if member.status != MemberStatus::Active {
        return Err(AppError::Forbidden("Account is suspended".to_string()));
    }

    verify_password(&req.password, &member.password_hash)?;

    // Resolve tenant_id from the organization
    let org = state
        .org_repo
        .find_by_id(member.org_id)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::internal("Organization not found for member"))?;

    build_session_response(&state, &member, &org)
}

/// POST /portal/v1/auth/logout
///
/// Clears session and CSRF cookies.
async fn logout() -> Response {
    let clear_session = format!(
        "{}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0",
        "portal_session"
    );
    let clear_csrf = "csrf_token=; Path=/; SameSite=Lax; Max-Age=0".to_string();

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
        token: jwt.clone(),
        user: AuthUser {
            id: member.id,
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

    let session_cookie =
        format!("portal_session={jwt}; Path=/; HttpOnly; SameSite=Lax; Max-Age=86400");
    let csrf_cookie = format!("csrf_token={csrf_token}; Path=/; SameSite=Lax; Max-Age=86400");

    let mut response = axum::response::Json(auth_resp).into_response();
    let headers = response.headers_mut();
    headers.append(header::SET_COOKIE, session_cookie.parse().unwrap());
    headers.append(header::SET_COOKIE, csrf_cookie.parse().unwrap());

    Ok(response)
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

    fn test_state_with(
        org_repo: MockOrgRepository,
        member_repo: MockMemberRepository,
    ) -> Arc<PortalState> {
        Arc::new(PortalState {
            org_repo: Arc::new(org_repo),
            member_repo: Arc::new(member_repo),
            api_key_repo: Arc::new(MockApiKeyRepository::new()),
            dashboard_repo: Arc::new(MockDashboardRepository::new()),
            jwt_secret: "test-secret-key-at-least-32-chars-long!!".to_string(),
        })
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
            email: "test@example.com".to_string(),
            password_hash: hash_password("password123").unwrap(),
            role: MemberRole::Owner,
            status: MemberStatus::Active,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    fn signup_app(state: Arc<PortalState>) -> Router {
        Router::new()
            .route("/auth/signup", post(signup))
            .route("/auth/login", post(login))
            .route("/auth/logout", post(logout))
            .with_state(state)
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
            move |id, org_id_arg, email, password_hash, _role| {
                Ok(OrgMember {
                    id,
                    org_id: org_id_arg,
                    email,
                    password_hash,
                    role: MemberRole::Owner,
                    status: MemberStatus::Active,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                })
            },
        );

        let state = test_state_with(org_repo, member_repo);
        let app = signup_app(state);

        let body = serde_json::json!({
            "org_name": "Test Org",
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

        let state = test_state_with(org_repo, member_repo);
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

        let state = test_state_with(MockOrgRepository::new(), member_repo);
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

        let state = test_state_with(MockOrgRepository::new(), member_repo);
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
}
