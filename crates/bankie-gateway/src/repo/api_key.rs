use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::RepoError;
use crate::models::api_key::{ApiKey, KeyStatus};

// === Repository trait (for portal routes + mocking) ===

#[allow(clippy::too_many_arguments)]
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait ApiKeyRepository: Send + Sync {
    async fn create(
        &self,
        id: Uuid,
        org_id: Uuid,
        tenant_id: i32,
        name: String,
        key_prefix: String,
        key_hash: String,
        scopes: Vec<String>,
    ) -> Result<ApiKey, RepoError>;

    async fn find_by_id(&self, id: Uuid, org_id: Uuid) -> Result<Option<ApiKey>, RepoError>;

    async fn find_valid_by_hash(&self, key_hash: String) -> Result<Option<ApiKey>, RepoError>;

    async fn list_by_org(&self, org_id: Uuid) -> Result<Vec<ApiKey>, RepoError>;

    async fn update_status(
        &self,
        id: Uuid,
        status: KeyStatus,
        grace_expires_at: Option<DateTime<Utc>>,
    ) -> Result<Option<ApiKey>, RepoError>;
}

// === Resolved API key (for gateway middleware) ===

/// Resolved API key data cached in Redis and used by gateway middleware.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ResolvedApiKey {
    pub api_key_id: Uuid,
    pub org_id: Uuid,
    pub tenant_id: i32,
    pub scopes: Vec<String>,
    pub environment: String,
}

// === SQL query functions (for gateway middleware) ===

/// Internal row type for the resolved key query.
#[derive(Debug, sqlx::FromRow)]
struct ResolvedApiKeyRow {
    api_key_id: Uuid,
    org_id: Uuid,
    tenant_id: i32,
    scopes: serde_json::Value,
    environment: String,
}

impl From<ResolvedApiKeyRow> for ResolvedApiKey {
    fn from(row: ResolvedApiKeyRow) -> Self {
        let scopes: Vec<String> = serde_json::from_value(row.scopes).unwrap_or_default();
        ResolvedApiKey {
            api_key_id: row.api_key_id,
            org_id: row.org_id,
            tenant_id: row.tenant_id,
            scopes,
            environment: row.environment,
        }
    }
}

/// Find an active API key by its SHA-256 hash.
/// Joins with organizations to resolve tenant_id.
pub async fn find_by_hash(
    pool: &sqlx::PgPool,
    key_hash: &str,
) -> Result<Option<ResolvedApiKey>, sqlx::Error> {
    let row: Option<ResolvedApiKeyRow> = sqlx::query_as(
        r#"
        SELECT ak.id AS api_key_id, ak.org_id, o.tenant_id, ak.scopes, ak.environment
        FROM portal.api_keys ak
        JOIN portal.organizations o ON o.id = ak.org_id
        WHERE ak.key_hash = $1
          AND ak.status = 'active'
          AND (ak.grace_expires_at IS NULL OR ak.grace_expires_at > now())
        "#,
    )
    .bind(key_hash)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(ResolvedApiKey::from))
}
