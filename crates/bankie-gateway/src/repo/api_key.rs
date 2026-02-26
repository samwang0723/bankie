// JUSTIFICATION: CRUD functions for portal API routes — dev-2 scope will wire these to handlers.
#![allow(dead_code)]

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::api_key::ApiKey;

/// Row type for sqlx queries against portal.api_keys.
#[derive(Debug, sqlx::FromRow)]
pub struct ApiKeyRow {
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

impl From<ApiKeyRow> for ApiKey {
    fn from(row: ApiKeyRow) -> Self {
        ApiKey {
            id: row.id,
            org_id: row.org_id,
            key_prefix: row.key_prefix,
            key_hash: row.key_hash,
            key_hint: row.key_hint,
            name: row.name,
            environment: row.environment,
            scopes: row.scopes,
            status: row.status,
            rotated_from_id: row.rotated_from_id,
            grace_expires_at: row.grace_expires_at,
            expires_at: row.expires_at,
            last_used_at: row.last_used_at,
            created_at: row.created_at,
            revoked_at: row.revoked_at,
        }
    }
}

/// Resolved API key data cached in Redis and used by middleware.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ResolvedApiKey {
    pub api_key_id: Uuid,
    pub org_id: Uuid,
    pub tenant_id: i32,
    pub scopes: Vec<String>,
    pub environment: String,
}

/// Insert a new API key record.
pub async fn create_api_key(pool: &PgPool, key: &ApiKey) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO portal.api_keys
            (id, org_id, key_prefix, key_hash, key_hint, name, environment, scopes, status, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        "#,
    )
    .bind(key.id)
    .bind(key.org_id)
    .bind(&key.key_prefix)
    .bind(&key.key_hash)
    .bind(&key.key_hint)
    .bind(&key.name)
    .bind(&key.environment)
    .bind(&key.scopes)
    .bind(&key.status)
    .bind(key.created_at)
    .execute(pool)
    .await?;

    Ok(())
}

/// Find an active API key by its SHA-256 hash.
/// Joins with organizations to resolve tenant_id.
pub async fn find_by_hash(
    pool: &PgPool,
    key_hash: &str,
) -> Result<Option<ResolvedApiKey>, sqlx::Error> {
    let row: Option<ResolvedApiKeyRow> = sqlx::query_as(
        r#"
        SELECT ak.id AS api_key_id, ak.org_id, o.tenant_id, ak.scopes, ak.environment
        FROM portal.api_keys ak
        JOIN portal.organizations o ON o.id = ak.org_id
        WHERE ak.key_hash = $1
          AND ak.status = 'active'
          AND (ak.expires_at IS NULL OR ak.expires_at > now())
        "#,
    )
    .bind(key_hash)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(ResolvedApiKey::from))
}

/// List all API keys for an organization.
pub async fn list_by_org(pool: &PgPool, org_id: Uuid) -> Result<Vec<ApiKey>, sqlx::Error> {
    let rows: Vec<ApiKeyRow> = sqlx::query_as(
        r#"
        SELECT id, org_id, key_prefix, key_hash, key_hint, name, environment,
               scopes, status, rotated_from_id, grace_expires_at, expires_at,
               last_used_at, created_at, revoked_at
        FROM portal.api_keys
        WHERE org_id = $1
        ORDER BY created_at DESC
        "#,
    )
    .bind(org_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(ApiKey::from).collect())
}

/// Rotate an API key: mark old key with grace period, insert new key with reference to old.
pub async fn rotate_api_key(
    pool: &PgPool,
    old_key_id: Uuid,
    new_key: &ApiKey,
    grace_period_secs: i64,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;

    // Set grace expiry on old key
    sqlx::query(
        r#"
        UPDATE portal.api_keys
        SET grace_expires_at = now() + make_interval(secs => $1),
            status = 'rotating'
        WHERE id = $2
        "#,
    )
    .bind(grace_period_secs as f64)
    .bind(old_key_id)
    .execute(&mut *tx)
    .await?;

    // Insert new key with reference to rotated_from
    sqlx::query(
        r#"
        INSERT INTO portal.api_keys
            (id, org_id, key_prefix, key_hash, key_hint, name, environment, scopes, status, rotated_from_id, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        "#,
    )
    .bind(new_key.id)
    .bind(new_key.org_id)
    .bind(&new_key.key_prefix)
    .bind(&new_key.key_hash)
    .bind(&new_key.key_hint)
    .bind(&new_key.name)
    .bind(&new_key.environment)
    .bind(&new_key.scopes)
    .bind(&new_key.status)
    .bind(old_key_id)
    .bind(new_key.created_at)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

/// Revoke an API key by setting status and revoked_at.
pub async fn revoke_api_key(pool: &PgPool, key_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE portal.api_keys
        SET status = 'revoked', revoked_at = now()
        WHERE id = $1
        "#,
    )
    .bind(key_id)
    .execute(pool)
    .await?;

    Ok(())
}

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
