use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::RepoError;
use crate::models::api_key::{ApiKey, KeyStatus};
use crate::models::dashboard::{AuditLogEntry, NewAuditLog};
use crate::models::member::{MemberRole, MemberStatus, OrgMember};
use crate::models::org::{OrgStatus, Organization};

// ─── Row types for sqlx deserialization ───

#[derive(sqlx::FromRow)]
struct OrgRow {
    id: Uuid,
    tenant_id: i32,
    name: String,
    slug: String,
    status: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<OrgRow> for Organization {
    fn from(row: OrgRow) -> Self {
        Organization {
            id: row.id,
            tenant_id: row.tenant_id,
            name: row.name,
            slug: row.slug,
            status: match row.status.as_str() {
                "suspended" => OrgStatus::Suspended,
                "closed" => OrgStatus::Closed,
                _ => OrgStatus::Active,
            },
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct MemberRow {
    id: Uuid,
    org_id: Uuid,
    email: String,
    password_hash: String,
    role: String,
    status: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<MemberRow> for OrgMember {
    fn from(row: MemberRow) -> Self {
        OrgMember {
            id: row.id,
            org_id: row.org_id,
            email: row.email,
            password_hash: row.password_hash,
            role: match row.role.as_str() {
                "owner" => MemberRole::Owner,
                "admin" => MemberRole::Admin,
                _ => MemberRole::Member,
            },
            status: match row.status.as_str() {
                "suspended" => MemberStatus::Suspended,
                _ => MemberStatus::Active,
            },
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct ApiKeyRow {
    id: Uuid,
    org_id: Uuid,
    tenant_id: i32,
    name: String,
    key_prefix: String,
    key_hash: String,
    scopes: serde_json::Value,
    status: String,
    grace_expires_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<ApiKeyRow> for ApiKey {
    fn from(row: ApiKeyRow) -> Self {
        let scopes: Vec<String> = serde_json::from_value(row.scopes).unwrap_or_default();
        ApiKey {
            id: row.id,
            org_id: row.org_id,
            tenant_id: row.tenant_id,
            name: row.name,
            key_prefix: row.key_prefix,
            key_hash: row.key_hash,
            scopes,
            status: match row.status.as_str() {
                "rotated" => KeyStatus::Rotated,
                "revoked" => KeyStatus::Revoked,
                _ => KeyStatus::Active,
            },
            grace_expires_at: row.grace_expires_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

fn status_str(status: &KeyStatus) -> &'static str {
    match status {
        KeyStatus::Active => "active",
        KeyStatus::Rotated => "rotated",
        KeyStatus::Revoked => "revoked",
    }
}

// ─── PgOrgRepository ───

pub struct PgOrgRepository {
    pool: PgPool,
}

impl PgOrgRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl super::org::OrgRepository for PgOrgRepository {
    async fn create(
        &self,
        id: Uuid,
        _tenant_id: i32,
        name: String,
        slug: String,
    ) -> Result<Organization, RepoError> {
        // tenant_id is auto-assigned by the portal.tenant_id_seq sequence
        let row: OrgRow = sqlx::query_as(
            r#"
            INSERT INTO portal.organizations (id, name, slug)
            VALUES ($1, $2, $3)
            RETURNING id, tenant_id, name, slug, status, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(&name)
        .bind(&slug)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.into())
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Organization>, RepoError> {
        let row: Option<OrgRow> = sqlx::query_as(
            r#"
            SELECT id, tenant_id, name, slug, status, created_at, updated_at
            FROM portal.organizations
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.map(Organization::from))
    }

    async fn find_by_slug(&self, slug: String) -> Result<Option<Organization>, RepoError> {
        let row: Option<OrgRow> = sqlx::query_as(
            r#"
            SELECT id, tenant_id, name, slug, status, created_at, updated_at
            FROM portal.organizations
            WHERE slug = $1
            "#,
        )
        .bind(&slug)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.map(Organization::from))
    }

    async fn update(
        &self,
        id: Uuid,
        name: Option<String>,
        status: Option<OrgStatus>,
    ) -> Result<Option<Organization>, RepoError> {
        let status_str = status.map(|s| match s {
            OrgStatus::Active => "active",
            OrgStatus::Suspended => "suspended",
            OrgStatus::Closed => "closed",
        });

        let row: Option<OrgRow> = sqlx::query_as(
            r#"
            UPDATE portal.organizations
            SET name = COALESCE($2, name),
                status = COALESCE($3, status),
                updated_at = now()
            WHERE id = $1
            RETURNING id, tenant_id, name, slug, status, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(name)
        .bind(status_str)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.map(Organization::from))
    }
}

// ─── PgMemberRepository ───

pub struct PgMemberRepository {
    pool: PgPool,
}

impl PgMemberRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl super::member::MemberRepository for PgMemberRepository {
    async fn create(
        &self,
        id: Uuid,
        org_id: Uuid,
        email: String,
        password_hash: String,
        role: String,
    ) -> Result<OrgMember, RepoError> {
        let row: MemberRow = sqlx::query_as(
            r#"
            INSERT INTO portal.org_members (id, org_id, email, password_hash, role, status)
            VALUES ($1, $2, $3, $4, $5, 'active')
            RETURNING id, org_id, email, password_hash, role, status, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(org_id)
        .bind(&email)
        .bind(&password_hash)
        .bind(&role)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| {
            if e.to_string().contains("duplicate key")
                || e.to_string().contains("unique constraint")
            {
                RepoError::Conflict("Email already registered".to_string())
            } else {
                RepoError::Database(e.to_string())
            }
        })?;

        Ok(row.into())
    }

    async fn find_by_email(&self, email: String) -> Result<Option<OrgMember>, RepoError> {
        let row: Option<MemberRow> = sqlx::query_as(
            r#"
            SELECT id, org_id, email, password_hash, role, status, created_at, updated_at
            FROM portal.org_members
            WHERE email = $1
            "#,
        )
        .bind(&email)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.map(OrgMember::from))
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<OrgMember>, RepoError> {
        let row: Option<MemberRow> = sqlx::query_as(
            r#"
            SELECT id, org_id, email, password_hash, role, status, created_at, updated_at
            FROM portal.org_members
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.map(OrgMember::from))
    }
}

// ─── PgApiKeyRepository ───

pub struct PgApiKeyRepository {
    pool: PgPool,
}

impl PgApiKeyRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl super::api_key::ApiKeyRepository for PgApiKeyRepository {
    async fn create(
        &self,
        id: Uuid,
        org_id: Uuid,
        tenant_id: i32,
        name: String,
        key_prefix: String,
        key_hash: String,
        scopes: Vec<String>,
    ) -> Result<ApiKey, RepoError> {
        let scopes_json = serde_json::to_value(&scopes).unwrap_or_default();

        let row: ApiKeyRow = sqlx::query_as(
            r#"
            INSERT INTO portal.api_keys (id, org_id, tenant_id, name, key_prefix, key_hash, scopes)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING id, org_id, tenant_id, name, key_prefix, key_hash, scopes,
                      status, grace_expires_at, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(org_id)
        .bind(tenant_id)
        .bind(&name)
        .bind(&key_prefix)
        .bind(&key_hash)
        .bind(&scopes_json)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.into())
    }

    async fn find_by_id(&self, id: Uuid, org_id: Uuid) -> Result<Option<ApiKey>, RepoError> {
        let row: Option<ApiKeyRow> = sqlx::query_as(
            r#"
            SELECT id, org_id, tenant_id, name, key_prefix, key_hash, scopes,
                   status, grace_expires_at, created_at, updated_at
            FROM portal.api_keys
            WHERE id = $1 AND org_id = $2
            "#,
        )
        .bind(id)
        .bind(org_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.map(ApiKey::from))
    }

    async fn find_valid_by_hash(&self, key_hash: String) -> Result<Option<ApiKey>, RepoError> {
        let row: Option<ApiKeyRow> = sqlx::query_as(
            r#"
            SELECT id, org_id, tenant_id, name, key_prefix, key_hash, scopes,
                   status, grace_expires_at, created_at, updated_at
            FROM portal.api_keys
            WHERE key_hash = $1 AND status = 'active'
            "#,
        )
        .bind(&key_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.map(ApiKey::from))
    }

    async fn list_by_org(&self, org_id: Uuid) -> Result<Vec<ApiKey>, RepoError> {
        let rows: Vec<ApiKeyRow> = sqlx::query_as(
            r#"
            SELECT id, org_id, tenant_id, name, key_prefix, key_hash, scopes,
                   status, grace_expires_at, created_at, updated_at
            FROM portal.api_keys
            WHERE org_id = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(org_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(rows.into_iter().map(ApiKey::from).collect())
    }

    async fn update_status(
        &self,
        id: Uuid,
        status: KeyStatus,
        grace_expires_at: Option<DateTime<Utc>>,
    ) -> Result<Option<ApiKey>, RepoError> {
        let row: Option<ApiKeyRow> = sqlx::query_as(
            r#"
            UPDATE portal.api_keys
            SET status = $2, grace_expires_at = $3, updated_at = now()
            WHERE id = $1
            RETURNING id, org_id, tenant_id, name, key_prefix, key_hash, scopes,
                      status, grace_expires_at, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(status_str(&status))
        .bind(grace_expires_at)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.map(ApiKey::from))
    }
}

// ─── PgDashboardRepository ───

#[derive(sqlx::FromRow)]
struct AuditLogRow {
    id: i64,
    org_id: Option<Uuid>,
    actor_id: Option<Uuid>,
    action: String,
    resource_type: String,
    resource_id: Option<String>,
    changes: Option<serde_json::Value>,
    created_at: DateTime<Utc>,
}

impl From<AuditLogRow> for AuditLogEntry {
    fn from(row: AuditLogRow) -> Self {
        AuditLogEntry {
            id: row.id,
            org_id: row.org_id.unwrap_or_default(),
            actor_id: row.actor_id.unwrap_or_default(),
            action: row.action,
            resource_type: row.resource_type,
            resource_id: row.resource_id,
            changes: row.changes,
            created_at: row.created_at,
        }
    }
}

pub struct PgDashboardRepository {
    pool: PgPool,
}

impl PgDashboardRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl super::dashboard::DashboardRepository for PgDashboardRepository {
    async fn count_api_calls_since(
        &self,
        org_id: Uuid,
        since: DateTime<Utc>,
    ) -> Result<i64, RepoError> {
        // Count API logs for keys belonging to this org since the given timestamp.
        let row: (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*) as count
            FROM portal.api_logs al
            JOIN portal.api_keys ak ON ak.id = al.api_key_id
            WHERE ak.org_id = $1 AND al.created_at >= $2
            "#,
        )
        .bind(org_id)
        .bind(since)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.0)
    }

    async fn list_recent_activity(
        &self,
        org_id: Uuid,
        limit: i64,
    ) -> Result<Vec<AuditLogEntry>, RepoError> {
        let rows: Vec<AuditLogRow> = sqlx::query_as(
            r#"
            SELECT id, org_id, actor_id, action, resource_type, resource_id, changes, created_at
            FROM portal.audit_logs
            WHERE org_id = $1
            ORDER BY created_at DESC
            LIMIT $2
            "#,
        )
        .bind(org_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(rows.into_iter().map(AuditLogEntry::from).collect())
    }

    async fn insert_audit_log(&self, entry: NewAuditLog) -> Result<(), RepoError> {
        sqlx::query(
            r#"
            INSERT INTO portal.audit_logs (org_id, actor_id, action, resource_type, resource_id, changes, client_ip)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(entry.org_id)
        .bind(entry.actor_id)
        .bind(&entry.action)
        .bind(&entry.resource_type)
        .bind(entry.resource_id.as_deref())
        .bind(&entry.changes)
        .bind(entry.client_ip.as_deref())
        .execute(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(())
    }
}
