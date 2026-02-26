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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MemberStatus {
    Active,
    Suspended,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrgMember {
    pub id: Uuid,
    pub org_id: Uuid,
    pub email: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub role: MemberRole,
    pub status: MemberStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
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
    fn test_password_hash_not_serialized() {
        let member = OrgMember {
            id: Uuid::new_v4(),
            org_id: Uuid::new_v4(),
            email: "test@example.com".to_string(),
            password_hash: "secret_hash".to_string(),
            role: MemberRole::Owner,
            status: MemberStatus::Active,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let json = serde_json::to_string(&member).unwrap();
        assert!(!json.contains("secret_hash"));
        assert!(!json.contains("password_hash"));
    }
}
