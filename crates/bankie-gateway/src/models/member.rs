use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MemberRole {
    Owner,
    Admin,
    Member,
}

impl MemberRole {
    /// Returns true if this role can manage members (invite, remove, change role).
    pub fn can_manage_members(&self) -> bool {
        matches!(self, MemberRole::Owner | MemberRole::Admin)
    }

    /// Returns true if this role can manage API keys (create, rotate, revoke).
    pub fn can_manage_api_keys(&self) -> bool {
        matches!(self, MemberRole::Owner | MemberRole::Admin)
    }

    /// Returns true if this role can modify organization settings.
    pub fn can_manage_org(&self) -> bool {
        matches!(self, MemberRole::Owner | MemberRole::Admin)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MemberStatus {
    Active,
    Pending,
    Suspended,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrgMember {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    pub email: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub role: MemberRole,
    pub status: MemberStatus,
    #[serde(skip_serializing)]
    pub invite_token_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invite_expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Request to invite a new member to the organization.
#[derive(Debug, Deserialize)]
pub struct InviteMemberRequest {
    pub email: String,
    pub role: String,
    #[serde(default)]
    pub name: String,
}

/// Response after inviting a member, includes the one-time invite link.
#[derive(Debug, Serialize)]
pub struct InviteMemberResponse {
    pub member: OrgMember,
    pub invite_link: String,
}

/// Request to accept an invite.
#[derive(Debug, Deserialize)]
pub struct AcceptInviteRequest {
    pub token: String,
    pub password: String,
    #[serde(default)]
    pub name: String,
}

/// Info returned when validating an invite token.
#[derive(Debug, Serialize)]
pub struct InviteInfo {
    pub email: String,
    pub org_name: String,
    pub role: MemberRole,
}

/// Generate a 32-byte random invite token (hex-encoded, 64 chars).
pub fn generate_invite_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
    hex::encode(bytes)
}

/// SHA-256 hash an invite token.
pub fn hash_invite_token(token: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

/// Request to change a member's role.
#[derive(Debug, Deserialize)]
pub struct UpdateRoleRequest {
    pub role: String,
}

/// Parse a role string into a MemberRole enum.
/// Returns None for invalid roles or if "owner" is specified (owner cannot be assigned).
pub fn parse_assignable_role(role: &str) -> Option<MemberRole> {
    match role {
        "admin" => Some(MemberRole::Admin),
        "member" => Some(MemberRole::Member),
        _ => None,
    }
}

/// Parse a session role string into a MemberRole enum.
pub fn parse_role(role: &str) -> MemberRole {
    match role {
        "owner" => MemberRole::Owner,
        "admin" => MemberRole::Admin,
        _ => MemberRole::Member,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_member_role_serialization() {
        let role = MemberRole::Owner;
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(json, "\"owner\"");
    }

    #[test]
    fn test_member_role_admin_serialization() {
        let role = MemberRole::Admin;
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(json, "\"admin\"");
    }

    #[test]
    fn test_password_hash_not_serialized() {
        let member = OrgMember {
            id: Uuid::new_v4(),
            org_id: Uuid::new_v4(),
            name: "Test User".to_string(),
            email: "test@example.com".to_string(),
            password_hash: "secret_hash".to_string(),
            role: MemberRole::Owner,
            status: MemberStatus::Active,
            invite_token_hash: None,
            invite_expires_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let json = serde_json::to_string(&member).unwrap();
        assert!(!json.contains("secret_hash"));
        assert!(!json.contains("password_hash"));
    }

    #[test]
    fn test_invite_token_hash_not_serialized() {
        let member = OrgMember {
            id: Uuid::new_v4(),
            org_id: Uuid::new_v4(),
            name: "Test User".to_string(),
            email: "test@example.com".to_string(),
            password_hash: "hash".to_string(),
            role: MemberRole::Member,
            status: MemberStatus::Pending,
            invite_token_hash: Some("secret_token_hash".to_string()),
            invite_expires_at: Some(Utc::now()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let json = serde_json::to_string(&member).unwrap();
        assert!(!json.contains("secret_token_hash"));
        assert!(!json.contains("invite_token_hash"));
    }

    #[test]
    fn test_invite_expires_at_hidden_when_none() {
        let member = OrgMember {
            id: Uuid::new_v4(),
            org_id: Uuid::new_v4(),
            name: "Test User".to_string(),
            email: "test@example.com".to_string(),
            password_hash: "hash".to_string(),
            role: MemberRole::Member,
            status: MemberStatus::Active,
            invite_token_hash: None,
            invite_expires_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let json = serde_json::to_string(&member).unwrap();
        assert!(!json.contains("invite_expires_at"));
    }

    #[test]
    fn test_generate_invite_token_format() {
        let token = generate_invite_token();
        assert_eq!(token.len(), 64); // 32 bytes hex-encoded
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_generate_invite_token_uniqueness() {
        let t1 = generate_invite_token();
        let t2 = generate_invite_token();
        assert_ne!(t1, t2);
    }

    #[test]
    fn test_hash_invite_token_deterministic() {
        let token = "abc123";
        let h1 = hash_invite_token(token);
        let h2 = hash_invite_token(token);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_hash_invite_token_different_inputs() {
        let h1 = hash_invite_token("token1");
        let h2 = hash_invite_token("token2");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_pending_status_serialization() {
        let status = MemberStatus::Pending;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"pending\"");
    }

    #[test]
    fn test_parse_assignable_role() {
        assert_eq!(parse_assignable_role("admin"), Some(MemberRole::Admin));
        assert_eq!(parse_assignable_role("member"), Some(MemberRole::Member));
        assert_eq!(parse_assignable_role("owner"), None);
        assert_eq!(parse_assignable_role("invalid"), None);
    }

    #[test]
    fn test_parse_role() {
        assert_eq!(parse_role("owner"), MemberRole::Owner);
        assert_eq!(parse_role("admin"), MemberRole::Admin);
        assert_eq!(parse_role("member"), MemberRole::Member);
        assert_eq!(parse_role("anything"), MemberRole::Member);
    }

    #[test]
    fn test_role_permissions() {
        assert!(MemberRole::Owner.can_manage_members());
        assert!(MemberRole::Owner.can_manage_api_keys());
        assert!(MemberRole::Owner.can_manage_org());

        assert!(MemberRole::Admin.can_manage_members());
        assert!(MemberRole::Admin.can_manage_api_keys());
        assert!(MemberRole::Admin.can_manage_org());

        assert!(!MemberRole::Member.can_manage_members());
        assert!(!MemberRole::Member.can_manage_api_keys());
        assert!(!MemberRole::Member.can_manage_org());
    }
}
