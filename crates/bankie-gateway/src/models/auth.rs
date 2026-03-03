use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::member::MemberRole;

#[derive(Debug, Deserialize, ToSchema)]
pub struct SignupRequest {
    pub org_name: String,
    pub name: String,
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionClaims {
    pub sub: String,
    pub name: String,
    pub org_id: String,
    pub tenant_id: i32,
    pub role: String,
    pub csrf: String,
    pub exp: usize,
    /// JWT ID for server-side session invalidation via Redis blocklist.
    #[serde(default)]
    pub jti: String,
}

/// Auth response matching SPA's expected format.
///
/// Note: JWT is delivered ONLY via HttpOnly cookie, never in the response body.
#[derive(Debug, Serialize, ToSchema)]
pub struct AuthResponse {
    pub user: AuthUser,
    pub organization: AuthOrganization,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AuthUser {
    pub id: Uuid,
    pub name: String,
    pub email: String,
    pub role: MemberRole,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AuthOrganization {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub environment: String,
    pub created_at: DateTime<Utc>,
}
