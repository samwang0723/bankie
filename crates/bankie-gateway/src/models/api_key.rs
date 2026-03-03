use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

// === Key Status ===

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum KeyStatus {
    Active,
    Rotated,
    Revoked,
}

/// Valid API key scopes for the portal.
pub const VALID_SCOPES: &[&str] = &[
    "accounts:read",
    "accounts:write",
    "ledgers:read",
    "ledgers:write",
    "transactions:read",
    "reports:read",
    "house_accounts:read",
    "house_accounts:write",
];

// === API key environment ===

/// API key environment (live vs test).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    Live,
    Test,
}

impl Environment {
    /// Returns the prefix string for this environment.
    pub fn prefix(&self) -> &'static str {
        match self {
            Environment::Live => "bnk_live_",
            Environment::Test => "bnk_test_",
        }
    }
}

impl std::str::FromStr for Environment {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "live" => Ok(Environment::Live),
            "test" => Ok(Environment::Test),
            _ => Err(format!("unknown environment: {s}")),
        }
    }
}

// === ApiKey model ===

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ApiKey {
    pub id: Uuid,
    pub org_id: Uuid,
    pub tenant_id: i32,
    pub name: String,
    pub key_prefix: String,
    #[serde(skip_serializing)]
    #[schema(ignore)]
    pub key_hash: String,
    pub scopes: Vec<String>,
    pub status: KeyStatus,
    pub grace_expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// === Request/Response DTOs ===

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateKeyRequest {
    pub name: String,
    #[serde(default)]
    pub environment: Option<String>,
    pub scopes: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CreateKeyResponse {
    pub id: Uuid,
    pub name: String,
    pub key_prefix: String,
    pub raw_key: String,
    pub scopes: Vec<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct KeyListItem {
    pub id: Uuid,
    pub name: String,
    pub key_prefix: String,
    pub scopes: Vec<String>,
    pub status: KeyStatus,
    pub grace_expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RotateKeyResponse {
    pub new_key: CreateKeyResponse,
    pub old_key_id: Uuid,
    pub grace_expires_at: DateTime<Utc>,
}

// === Key generation & hashing ===

/// Generate a random API key with the `bk_live_` prefix.
pub fn generate_raw_key() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let random_part: String = (0..48)
        .map(|_| {
            let idx = rng.gen_range(0..36u8);
            if idx < 10 {
                (b'0' + idx) as char
            } else {
                (b'a' + idx - 10) as char
            }
        })
        .collect();
    format!("bk_live_{random_part}")
}

/// Extract the display prefix from a raw key (first 16 chars).
pub fn key_prefix(raw_key: &str) -> String {
    raw_key.chars().take(16).collect()
}

/// Hash a raw API key with SHA-256.
pub fn hash_key(raw_key: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(raw_key.as_bytes());
    hex::encode(hasher.finalize())
}

/// Alias for gateway middleware compatibility.
pub fn hash_api_key(raw_key: &str) -> String {
    hash_key(raw_key)
}

/// Extract the last 4 characters of the raw key as a hint.
pub fn extract_hint(raw_key: &str) -> String {
    let len = raw_key.len();
    if len >= 4 {
        raw_key[len - 4..].to_string()
    } else {
        raw_key.to_string()
    }
}

/// Validate that all requested scopes are in the allowed set.
pub fn validate_scopes(scopes: &[String]) -> Result<(), Vec<String>> {
    let invalid: Vec<String> = scopes
        .iter()
        .filter(|s| !VALID_SCOPES.contains(&s.as_str()))
        .cloned()
        .collect();
    if invalid.is_empty() {
        Ok(())
    } else {
        Err(invalid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Key generation tests ---

    #[test]
    fn test_generate_raw_key_format() {
        let key = generate_raw_key();
        assert!(key.starts_with("bk_live_"));
        assert_eq!(key.len(), 56); // "bk_live_" (8) + 48
    }

    #[test]
    fn test_generate_raw_key_uniqueness() {
        let k1 = generate_raw_key();
        let k2 = generate_raw_key();
        assert_ne!(k1, k2);
    }

    #[test]
    fn test_key_prefix_extraction() {
        let raw = "bk_live_abcdefghijklmnop1234567890abcdefghijklmnop12";
        let prefix = key_prefix(raw);
        assert_eq!(prefix, "bk_live_abcdefgh");
    }

    // --- Hash tests ---

    #[test]
    fn test_hash_key_deterministic() {
        let key = "bk_live_testkey123";
        let h1 = hash_key(key);
        let h2 = hash_key(key);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_hash_key_different_inputs() {
        let h1 = hash_key("key1");
        let h2 = hash_key("key2");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_hash_api_key_is_sha256_hex() {
        let key = "bnk_live_testkey";
        let hash = hash_api_key(key);
        assert_eq!(hash.len(), 64, "SHA-256 hex should be 64 chars");
        for ch in hash.chars() {
            assert!(ch.is_ascii_hexdigit(), "Hash should be hex, got: {}", ch);
        }
    }

    // --- Hint tests ---

    #[test]
    fn test_extract_hint_last_4_chars() {
        let hint = extract_hint("bnk_live_abcdefghijklmnop");
        assert_eq!(hint, "mnop", "Hint should be last 4 chars");
    }

    #[test]
    fn test_extract_hint_short_key() {
        let hint = extract_hint("ab");
        assert_eq!(hint, "ab", "Short keys return the whole string");
    }

    // --- Scope validation tests ---

    #[test]
    fn test_validate_scopes_valid() {
        let scopes = vec!["accounts:read".to_string(), "ledgers:read".to_string()];
        assert!(validate_scopes(&scopes).is_ok());
    }

    #[test]
    fn test_validate_scopes_invalid() {
        let scopes = vec!["accounts:read".to_string(), "invalid:scope".to_string()];
        let result = validate_scopes(&scopes);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), vec!["invalid:scope".to_string()]);
    }

    #[test]
    fn test_validate_scopes_empty() {
        let scopes: Vec<String> = vec![];
        assert!(validate_scopes(&scopes).is_ok());
    }

    // --- Serialization tests ---

    #[test]
    fn test_key_status_serialization() {
        let status = KeyStatus::Active;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"active\"");
    }

    #[test]
    fn test_key_hash_not_serialized() {
        let key = ApiKey {
            id: Uuid::new_v4(),
            org_id: Uuid::new_v4(),
            tenant_id: 1,
            name: "test".to_string(),
            key_prefix: "bk_live_abc".to_string(),
            key_hash: "secret_hash_value".to_string(),
            scopes: vec!["accounts:read".to_string()],
            status: KeyStatus::Active,
            grace_expires_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let json = serde_json::to_string(&key).unwrap();
        assert!(!json.contains("secret_hash_value"));
        assert!(!json.contains("key_hash"));
    }

    // --- Environment tests ---

    #[test]
    fn test_environment_prefix() {
        assert_eq!(Environment::Live.prefix(), "bnk_live_");
        assert_eq!(Environment::Test.prefix(), "bnk_test_");
    }

    #[test]
    fn test_environment_from_str() {
        assert_eq!("live".parse::<Environment>(), Ok(Environment::Live));
        assert_eq!("test".parse::<Environment>(), Ok(Environment::Test));
        assert!("unknown".parse::<Environment>().is_err());
    }
}
