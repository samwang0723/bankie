// JUSTIFICATION: Public API for portal key management — route handlers (dev-2 scope) will use these.
#![allow(dead_code)]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Base62 charset for API key generation.
const BASE62_CHARSET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Length of the random portion of an API key.
const KEY_RANDOM_LENGTH: usize = 32;

/// API key environment prefixes.
const LIVE_PREFIX: &str = "bnk_live_";
const TEST_PREFIX: &str = "bnk_test_";

/// Represents an API key stored in the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: Uuid,
    pub org_id: Uuid,
    pub key_prefix: String,
    pub key_hash: String,
    pub key_hint: String,
    pub name: String,
    pub environment: String,
    pub scopes: serde_json::Value,
    pub status: String,
    pub rotated_from_id: Option<Uuid>,
    pub grace_expires_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

/// Result of generating a new API key. Contains the raw key (shown once) and the DB record.
#[derive(Debug)]
pub struct GeneratedApiKey {
    /// The full raw API key (e.g., "bnk_live_Ab3x..."). Only returned at creation time.
    pub raw_key: String,
    /// The database record (contains hash, not raw key).
    pub record: ApiKey,
}

/// API key environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    Live,
    Test,
}

impl Environment {
    /// Returns the prefix string for this environment.
    pub fn prefix(&self) -> &'static str {
        match self {
            Environment::Live => LIVE_PREFIX,
            Environment::Test => TEST_PREFIX,
        }
    }

    /// Parse from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "live" => Some(Environment::Live),
            "test" => Some(Environment::Test),
            _ => None,
        }
    }
}

/// Generate a new API key with the specified environment and metadata.
pub fn generate_api_key(
    org_id: Uuid,
    name: &str,
    environment: Environment,
    scopes: Vec<String>,
) -> GeneratedApiKey {
    let prefix = environment.prefix();
    let random_part = generate_base62(KEY_RANDOM_LENGTH);
    let raw_key = format!("{}{}", prefix, random_part);

    let key_hash = hash_api_key(&raw_key);
    let key_hint = extract_hint(&raw_key);

    let record = ApiKey {
        id: Uuid::new_v4(),
        org_id,
        key_prefix: prefix.to_string(),
        key_hash,
        key_hint,
        name: name.to_string(),
        environment: match environment {
            Environment::Live => "live".to_string(),
            Environment::Test => "test".to_string(),
        },
        scopes: serde_json::to_value(scopes).unwrap_or_default(),
        status: "active".to_string(),
        rotated_from_id: None,
        grace_expires_at: None,
        expires_at: None,
        last_used_at: None,
        created_at: Utc::now(),
        revoked_at: None,
    };

    GeneratedApiKey { raw_key, record }
}

/// SHA-256 hash an API key, returning the hex-encoded digest.
pub fn hash_api_key(raw_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw_key.as_bytes());
    hex::encode(hasher.finalize())
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

/// Generate a random base62 string of the given length.
fn generate_base62(length: usize) -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..length)
        .map(|_| {
            let idx = rng.gen_range(0..BASE62_CHARSET.len());
            BASE62_CHARSET[idx] as char
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_api_key_live_prefix() {
        let org_id = Uuid::new_v4();
        let generated = generate_api_key(
            org_id,
            "test-key",
            Environment::Live,
            vec!["bank-account:read".to_string()],
        );

        assert!(
            generated.raw_key.starts_with("bnk_live_"),
            "Live key should start with bnk_live_ prefix"
        );
        assert_eq!(generated.record.environment, "live");
        assert_eq!(generated.record.key_prefix, "bnk_live_");
    }

    #[test]
    fn test_generate_api_key_test_prefix() {
        let org_id = Uuid::new_v4();
        let generated = generate_api_key(
            org_id,
            "test-key",
            Environment::Test,
            vec!["bank-account:read".to_string()],
        );

        assert!(
            generated.raw_key.starts_with("bnk_test_"),
            "Test key should start with bnk_test_ prefix"
        );
        assert_eq!(generated.record.environment, "test");
        assert_eq!(generated.record.key_prefix, "bnk_test_");
    }

    #[test]
    fn test_generate_api_key_length() {
        let org_id = Uuid::new_v4();
        let generated = generate_api_key(org_id, "test-key", Environment::Live, vec![]);

        // prefix (9 chars "bnk_live_") + 32 base62 chars = 41
        assert_eq!(
            generated.raw_key.len(),
            LIVE_PREFIX.len() + KEY_RANDOM_LENGTH,
            "Key should be prefix + 32 random chars"
        );
    }

    #[test]
    fn test_generate_api_key_base62_chars_only() {
        let org_id = Uuid::new_v4();
        let generated = generate_api_key(org_id, "test-key", Environment::Live, vec![]);

        let random_part = &generated.raw_key[LIVE_PREFIX.len()..];
        for ch in random_part.chars() {
            assert!(
                ch.is_ascii_alphanumeric(),
                "Random portion should be base62 (alphanumeric), got: {}",
                ch
            );
        }
    }

    #[test]
    fn test_hash_api_key_deterministic() {
        let key = "bnk_live_abc123";
        let hash1 = hash_api_key(key);
        let hash2 = hash_api_key(key);
        assert_eq!(hash1, hash2, "Same key should produce same hash");
    }

    #[test]
    fn test_hash_api_key_is_sha256_hex() {
        let key = "bnk_live_testkey";
        let hash = hash_api_key(key);

        // SHA-256 hex digest is 64 chars
        assert_eq!(hash.len(), 64, "SHA-256 hex should be 64 chars");

        // All hex chars
        for ch in hash.chars() {
            assert!(ch.is_ascii_hexdigit(), "Hash should be hex, got: {}", ch);
        }
    }

    #[test]
    fn test_hash_api_key_different_keys_different_hashes() {
        let hash1 = hash_api_key("bnk_live_key1");
        let hash2 = hash_api_key("bnk_live_key2");
        assert_ne!(
            hash1, hash2,
            "Different keys should produce different hashes"
        );
    }

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

    #[test]
    fn test_generated_key_hint_matches_raw_key() {
        let org_id = Uuid::new_v4();
        let generated = generate_api_key(org_id, "test", Environment::Live, vec![]);
        let expected_hint = &generated.raw_key[generated.raw_key.len() - 4..];
        assert_eq!(generated.record.key_hint, expected_hint);
    }

    #[test]
    fn test_generated_key_hash_matches_raw_key() {
        let org_id = Uuid::new_v4();
        let generated = generate_api_key(org_id, "test", Environment::Live, vec![]);
        let expected_hash = hash_api_key(&generated.raw_key);
        assert_eq!(generated.record.key_hash, expected_hash);
    }

    #[test]
    fn test_generated_key_metadata() {
        let org_id = Uuid::new_v4();
        let scopes = vec!["bank-account:read".to_string(), "ledger:read".to_string()];
        let generated = generate_api_key(org_id, "my-api-key", Environment::Live, scopes.clone());

        assert_eq!(generated.record.org_id, org_id);
        assert_eq!(generated.record.name, "my-api-key");
        assert_eq!(generated.record.status, "active");
        assert!(generated.record.rotated_from_id.is_none());
        assert!(generated.record.revoked_at.is_none());

        let stored_scopes: Vec<String> = serde_json::from_value(generated.record.scopes).unwrap();
        assert_eq!(stored_scopes, scopes);
    }

    #[test]
    fn test_uniqueness_of_generated_keys() {
        let org_id = Uuid::new_v4();
        let key1 = generate_api_key(org_id, "k1", Environment::Live, vec![]);
        let key2 = generate_api_key(org_id, "k2", Environment::Live, vec![]);
        assert_ne!(
            key1.raw_key, key2.raw_key,
            "Each generation should be unique"
        );
        assert_ne!(key1.record.id, key2.record.id);
    }

    #[test]
    fn test_environment_prefix() {
        assert_eq!(Environment::Live.prefix(), "bnk_live_");
        assert_eq!(Environment::Test.prefix(), "bnk_test_");
    }

    #[test]
    fn test_environment_from_str() {
        assert_eq!(Environment::from_str("live"), Some(Environment::Live));
        assert_eq!(Environment::from_str("test"), Some(Environment::Test));
        assert_eq!(Environment::from_str("unknown"), None);
    }
}
