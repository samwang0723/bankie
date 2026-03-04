use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum OrgStatus {
    Active,
    Suspended,
    Closed,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Organization {
    pub id: Uuid,
    pub tenant_id: i32,
    pub name: String,
    pub slug: String,
    pub status: OrgStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateOrgRequest {
    pub name: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateOrgRequest {
    pub name: Option<String>,
    pub status: Option<OrgStatus>,
}

/// Convert a name into a URL-friendly slug.
pub fn slugify(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slugify_simple() {
        assert_eq!(slugify("My Company"), "my-company");
    }

    #[test]
    fn test_slugify_special_chars() {
        assert_eq!(slugify("Hello & World!"), "hello-world");
    }

    #[test]
    fn test_slugify_multiple_spaces() {
        assert_eq!(slugify("  lots   of   spaces  "), "lots-of-spaces");
    }

    #[test]
    fn test_slugify_already_slug() {
        assert_eq!(slugify("already-a-slug"), "already-a-slug");
    }

    #[test]
    fn test_org_status_serialization() {
        let status = OrgStatus::Active;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"active\"");
    }
}
