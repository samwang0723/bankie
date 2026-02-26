use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::member::MemberRole;

#[derive(Debug, Deserialize)]
pub struct SignupRequest {
    pub org_name: String,
    pub tenant_id: i32,
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

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub member_id: Uuid,
    pub org_id: Uuid,
    pub email: String,
    pub role: MemberRole,
}
