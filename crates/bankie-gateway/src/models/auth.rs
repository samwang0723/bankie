use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::member::MemberRole;

#[derive(Debug, Deserialize)]
pub struct SignupRequest {
    pub org_name: String,
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionClaims {
    pub sub: String,
    pub org_id: String,
    pub tenant_id: i32,
    pub role: String,
    pub csrf: String,
    pub exp: usize,
}

/// Auth response matching SPA's expected format:
/// `{ token, user: { id, email, role, created_at }, organization: { id, name, slug, environment, created_at } }`
#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: AuthUser,
    pub organization: AuthOrganization,
}

#[derive(Debug, Serialize)]
pub struct AuthUser {
    pub id: Uuid,
    pub email: String,
    pub role: MemberRole,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct AuthOrganization {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub environment: String,
    pub created_at: DateTime<Utc>,
}
